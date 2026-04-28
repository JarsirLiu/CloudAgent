use agent_core::{
    AgentContext, AgentSession, AgentTurnOutput, ChatModel, ExecutionPolicy, ModelRequest,
    ModelResponse, ToolCall, ToolEvent, ToolExecutor, ToolSpec,
};
use agent_tools::ToolRegistry;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use config::{AgentConfig, LlmConfig};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use storage::JsonSessionStore;
use tokio::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub use agent_core::{
    ApprovalDecision, ApprovalRequest, ConversationMessage, SessionSnapshot, SessionState,
    TurnEvent, TurnOutcome, TurnState,
};

pub fn crate_name() -> &'static str {
    "agent-runtime"
}

pub struct AgentRuntime {
    config: AgentConfig,
    context: AgentContext,
    policy: ExecutionPolicy,
    model: Arc<dyn ChatModel>,
    tools: Arc<dyn ToolExecutor>,
    sessions: Mutex<HashMap<String, AgentSession>>,
    store: JsonSessionStore,
    active_turns: Mutex<HashMap<String, String>>,
}

impl AgentRuntime {
    pub fn from_config(config: AgentConfig) -> Result<Self> {
        config.validate()?;
        let context = AgentContext {
            workspace_root: config.workspace_root.clone(),
            default_shell_timeout_ms: config.tools.default_shell_timeout_ms,
        };
        let policy = ExecutionPolicy::new(config.runtime.max_tool_roundtrips);
        let model = Arc::new(OpenAiCompatibleModel::new(config.llm.clone())?);
        let tools = Arc::new(ToolRegistry::new(config.tools.max_read_chars));
        let store = JsonSessionStore::new(config.runtime.session_store_dir.clone());

        Ok(Self {
            config,
            context,
            policy,
            model,
            tools,
            sessions: Mutex::new(HashMap::new()),
            store,
            active_turns: Mutex::new(HashMap::new()),
        })
    }

    pub async fn chat(&self, session_id: &str, user_input: &str) -> Result<AgentTurnOutput> {
        let outcome = self
            .run_turn_with_approval(session_id, user_input, |_request| async move {
                Ok(ApprovalDecision {
                    approved: false,
                    reason: Some(
                        "Mutating tools require an approval-capable client. Use the interactive cli."
                            .to_string(),
                    ),
                })
            })
            .await?;
        Ok(self.outcome_to_output(outcome))
    }

    pub async fn chat_with_approval<F, Fut>(
        &self,
        session_id: &str,
        user_input: &str,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        F: Fn(ApprovalRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ApprovalDecision>> + Send,
    {
        let outcome = self
            .run_turn_with_approval(session_id, user_input, approval)
            .await?;
        Ok(self.outcome_to_output(outcome))
    }

    pub async fn reset_session(&self, session_id: &str) -> Result<()> {
        self.sessions.lock().await.remove(session_id);
        self.active_turns.lock().await.remove(session_id);
        self.store.delete_session(session_id).await
    }

    pub async fn session_snapshot(&self, session_id: &str) -> Result<AgentSession> {
        if let Some(session) = self.sessions.lock().await.get(session_id).cloned() {
            return Ok(session);
        }
        if let Some(mut session) = self.store.load_session(session_id).await? {
            session.ensure_system_prompt(self.config.runtime.system_prompt.clone());
            return Ok(session);
        }
        Ok(AgentSession::new(
            session_id.to_string(),
            self.config.runtime.system_prompt.clone(),
        ))
    }

    pub fn default_session_id(&self) -> &str {
        &self.config.runtime.default_session_id
    }

    pub async fn session_state(&self, session_id: &str) -> Result<SessionSnapshot> {
        let session = self.session_snapshot(session_id).await?;
        let active_turn = self.active_turns.lock().await.get(session_id).cloned();
        Ok(SessionSnapshot {
            session_id: session_id.to_string(),
            session_state: if active_turn.is_some() {
                SessionState::Busy
            } else {
                SessionState::Idle
            },
            active_turn,
            message_count: session.messages.len(),
        })
    }

    async fn run_turn_with_approval<F, Fut>(
        &self,
        session_id: &str,
        user_input: &str,
        approval: F,
    ) -> Result<TurnOutcome>
    where
        F: Fn(ApprovalRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ApprovalDecision>> + Send,
    {
        let turn_id = next_turn_id();
        self.active_turns
            .lock()
            .await
            .insert(session_id.to_string(), turn_id.clone());

        let mut session = self.load_session(session_id).await?;
        session.push_user_message(user_input);

        let mut events = vec![TurnEvent::TurnStarted {
            turn_id: turn_id.clone(),
            session_id: session_id.to_string(),
            user_input: user_input.to_string(),
        }];
        let mut last_model_name = None;
        let tool_specs = self.tools.specs();

        let result: Result<TurnOutcome> = async {
            for _ in 0..self.policy.max_tool_roundtrips {
                events.push(TurnEvent::ModelRequestStarted {
                    turn_id: turn_id.clone(),
                    message_count: session.messages.len(),
                    tool_count: tool_specs.len(),
                });

                let response = self
                    .model
                    .complete(ModelRequest {
                        messages: session.messages.clone(),
                        tools: tool_specs.clone(),
                        temperature: self.config.llm.temperature,
                    })
                    .await?;

                last_model_name = response.model_name.clone();
                let tool_calls = response.tool_calls.clone();
                events.push(TurnEvent::ModelResponseReceived {
                    turn_id: turn_id.clone(),
                    model_name: response.model_name.clone(),
                    has_content: response.content.is_some(),
                    tool_call_count: tool_calls.len(),
                });

                if let Some(content) = response.content.clone() {
                    events.push(TurnEvent::AssistantMessage {
                        turn_id: turn_id.clone(),
                        content: content.clone(),
                    });
                }

                session.push_assistant_message(response.content.clone(), tool_calls.clone());

                if tool_calls.is_empty() {
                    let final_response = response
                        .content
                        .unwrap_or_else(|| "The model returned an empty response.".to_string());
                    events.push(TurnEvent::TurnCompleted {
                        turn_id: turn_id.clone(),
                        final_response: final_response.clone(),
                    });
                    return Ok(TurnOutcome {
                        turn_id: turn_id.clone(),
                        final_response,
                        events: events.clone(),
                        session,
                        model_name: last_model_name.clone(),
                        state: TurnState::Completed,
                    });
                }

                let tool_ctx = self.context.tool_context(session_id.to_string());
                for call in tool_calls {
                    events.push(TurnEvent::ToolCallRequested {
                        turn_id: turn_id.clone(),
                        call: call.clone(),
                    });

                    if let Some(spec) = tool_specs.iter().find(|spec| spec.name == call.name)
                        && spec.requires_approval
                    {
                        let request = ApprovalRequest {
                            turn_id: turn_id.clone(),
                            tool_call_id: call.id.clone(),
                            tool_name: call.name.clone(),
                            reason: format!(
                                "Tool `{}` can modify files or execute commands.",
                                call.name
                            ),
                            arguments_preview: summarize_arguments(&call.arguments),
                        };
                        events.push(TurnEvent::ApprovalRequested {
                            turn_id: turn_id.clone(),
                            request: request.clone(),
                        });
                        let decision = approval(request).await?;
                        events.push(TurnEvent::ApprovalResolved {
                            turn_id: turn_id.clone(),
                            tool_call_id: call.id.clone(),
                            approved: decision.approved,
                            reason: decision.reason.clone(),
                        });
                        if !decision.approved {
                            let reason = decision
                                .reason
                                .unwrap_or_else(|| "approval denied".to_string());
                            let result = agent_core::ToolResult {
                                tool_call_id: call.id.clone(),
                                name: call.name.clone(),
                                content: format!("Tool execution skipped: {reason}"),
                                summary: "tool execution skipped".to_string(),
                                is_error: true,
                            };
                            events.push(TurnEvent::ToolCallFailed {
                                turn_id: turn_id.clone(),
                                tool_call_id: call.id.clone(),
                                tool_name: call.name.clone(),
                                error: reason.clone(),
                            });
                            session.push_tool_result(result);
                            continue;
                        }
                    }

                    let result = self.tools.execute(call.clone(), &tool_ctx).await?;
                    if result.is_error {
                        events.push(TurnEvent::ToolCallFailed {
                            turn_id: turn_id.clone(),
                            tool_call_id: result.tool_call_id.clone(),
                            tool_name: result.name.clone(),
                            error: result.content.clone(),
                        });
                    } else {
                        events.push(TurnEvent::ToolCallCompleted {
                            turn_id: turn_id.clone(),
                            result: result.clone(),
                        });
                    }
                    session.push_tool_result(result);
                }
            }

            let final_response =
                "Reached the configured tool roundtrip limit before the model produced a final answer."
                    .to_string();
            session.push_assistant_message(Some(final_response.clone()), Vec::new());
            events.push(TurnEvent::TurnCompleted {
                turn_id: turn_id.clone(),
                final_response: final_response.clone(),
            });
            Ok(TurnOutcome {
                turn_id: turn_id.clone(),
                final_response,
                events: events.clone(),
                session,
                model_name: last_model_name.clone(),
                state: TurnState::Completed,
            })
        }
        .await;

        self.active_turns.lock().await.remove(session_id);

        match result {
            Ok(outcome) => {
                self.save_session(outcome.session.clone()).await?;
                Ok(outcome)
            }
            Err(err) => {
                let mut session = self.load_session(session_id).await?;
                session.push_assistant_message(
                    Some(format!("Turn failed: {err:#}")),
                    Vec::new(),
                );
                self.save_session(session.clone()).await?;
                let error_text = format!("{err:#}");
                events.push(TurnEvent::TurnFailed {
                    turn_id: turn_id.clone(),
                    error: error_text.clone(),
                });
                Ok(TurnOutcome {
                    turn_id,
                    final_response: format!("Turn failed: {error_text}"),
                    events,
                    session,
                    model_name: last_model_name,
                    state: TurnState::Failed,
                })
            }
        }
    }

    async fn load_session(&self, session_id: &str) -> Result<AgentSession> {
        if let Some(session) = self.sessions.lock().await.get(session_id).cloned() {
            return Ok(session);
        }

        let mut session = if let Some(session) = self.store.load_session(session_id).await? {
            session
        } else {
            AgentSession::new(
                session_id.to_string(),
                self.config.runtime.system_prompt.clone(),
            )
        };
        session.ensure_system_prompt(self.config.runtime.system_prompt.clone());
        self.sessions
            .lock()
            .await
            .insert(session_id.to_string(), session.clone());
        Ok(session)
    }

    async fn save_session(&self, session: AgentSession) -> Result<()> {
        self.store.save_session(&session).await?;
        self.sessions
            .lock()
            .await
            .insert(session.id.clone(), session);
        Ok(())
    }

    fn outcome_to_output(&self, outcome: TurnOutcome) -> AgentTurnOutput {
        let tool_events = outcome
            .events
            .iter()
            .filter_map(|event| match event {
                TurnEvent::ToolCallCompleted { result, .. } => Some(ToolEvent {
                    name: result.name.clone(),
                    summary: result.summary.clone(),
                    is_error: false,
                }),
                TurnEvent::ToolCallFailed {
                    tool_name, error, ..
                } => Some(ToolEvent {
                    name: tool_name.clone(),
                    summary: error.clone(),
                    is_error: true,
                }),
                _ => None,
            })
            .collect::<Vec<_>>();

        AgentTurnOutput {
            turn_id: outcome.turn_id,
            final_response: outcome.final_response,
            tool_events,
            events: outcome.events,
            model_name: outcome.model_name,
            total_messages: outcome.session.messages.len(),
            state: outcome.state,
        }
    }
}

struct OpenAiCompatibleModel {
    client: Client,
    config: LlmConfig,
}

impl OpenAiCompatibleModel {
    fn new(config: LlmConfig) -> Result<Self> {
        let client = Client::builder()
            .user_agent("cloudagent/0.1.0")
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { client, config })
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[async_trait]
impl ChatModel for OpenAiCompatibleModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let payload = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: request
                .messages
                .iter()
                .map(ChatApiMessage::from_message)
                .collect::<Result<Vec<_>>>()?,
            tools: request
                .tools
                .iter()
                .map(ChatToolSpec::from_spec)
                .collect(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: request.temperature,
        };

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await
            .context("failed to send LLM request")?;

        let status = response.status();
        let body = response.text().await.context("failed to read LLM body")?;
        if !status.is_success() {
            bail!("LLM request failed with status {status}: {body}");
        }

        let parsed: ChatCompletionResponse =
            serde_json::from_str(&body).context("failed to parse LLM response")?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("LLM response contained no choices"))?;

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|call| {
                let arguments = serde_json::from_str::<Value>(&call.function.arguments)
                    .unwrap_or_else(|_| Value::String(call.function.arguments.clone()));
                ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments,
                }
            })
            .collect();

        Ok(ModelResponse {
            content: choice.message.content,
            tool_calls,
            model_name: Some(parsed.model),
        })
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatApiMessage>,
    tools: Vec<ChatToolSpec>,
    tool_choice: String,
    parallel_tool_calls: bool,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl ChatApiMessage {
    fn from_message(message: &ConversationMessage) -> Result<Self> {
        match message {
            ConversationMessage::System { content } => Ok(Self {
                role: "system".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }),
            ConversationMessage::User { content } => Ok(Self {
                role: "user".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }),
            ConversationMessage::Assistant { content, tool_calls } => Ok(Self {
                role: "assistant".to_string(),
                content: content.clone(),
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(
                        tool_calls
                            .iter()
                            .map(ChatToolCall::from_internal)
                            .collect::<Result<Vec<_>>>()?,
                    )
                },
                tool_call_id: None,
                name: None,
            }),
            ConversationMessage::Tool {
                tool_call_id,
                name,
                content,
            } => Ok(Self {
                role: "tool".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: Some(tool_call_id.clone()),
                name: Some(name.clone()),
            }),
        }
    }
}

#[derive(Serialize)]
struct ChatToolSpec {
    #[serde(rename = "type")]
    kind: String,
    function: ChatToolFunctionSpec,
}

impl ChatToolSpec {
    fn from_spec(spec: &ToolSpec) -> Self {
        Self {
            kind: "function".to_string(),
            function: ChatToolFunctionSpec {
                name: spec.name.clone(),
                description: spec.description.clone(),
                parameters: spec.parameters.clone(),
            },
        }
    }
}

#[derive(Serialize)]
struct ChatToolFunctionSpec {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Serialize)]
struct ChatToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: ChatToolFunctionCall,
}

impl ChatToolCall {
    fn from_internal(call: &ToolCall) -> Result<Self> {
        Ok(Self {
            id: call.id.clone(),
            kind: "function".to_string(),
            function: ChatToolFunctionCall {
                name: call.name.clone(),
                arguments: serde_json::to_string(&call.arguments)?,
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ChatToolFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    model: String,
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatResponseToolCall>>,
}

#[derive(Deserialize)]
struct ChatResponseToolCall {
    id: String,
    function: ChatToolFunctionCall,
}

fn summarize_arguments(arguments: &Value) -> String {
    let rendered = serde_json::to_string(arguments).unwrap_or_else(|_| "<invalid-json>".to_string());
    if rendered.chars().count() > 240 {
        let truncated = rendered.chars().take(240).collect::<String>();
        format!("{truncated}...")
    } else {
        rendered
    }
}

fn next_turn_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("turn-{now}")
}
