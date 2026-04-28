mod state;
mod tasks;

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
use state::RuntimeState;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use storage::JsonSessionStore;
use tasks::{RegularTurnTask, RuntimeTask, TaskContext, TurnOutcome};
use tokio_util::sync::CancellationToken;

pub use agent_core::ConversationMessage;
pub use agent_protocol::{
    ApprovalDecision, ApprovalRequest, SessionSnapshot, SessionState, TurnEvent, TurnState,
};

const TURN_INTERRUPTED_ERROR: &str = "turn interrupted by client";

pub fn crate_name() -> &'static str {
    "agent-runtime"
}

pub struct AgentRuntime {
    config: AgentConfig,
    context: AgentContext,
    policy: ExecutionPolicy,
    model: Arc<dyn ChatModel>,
    tools: Arc<dyn ToolExecutor>,
    state: RuntimeState,
    store: JsonSessionStore,
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
            state: RuntimeState::new(),
            store,
        })
    }

    pub async fn chat(&self, session_id: &str, user_input: &str) -> Result<AgentTurnOutput> {
        let outcome = self
            .chat_with_approval_and_events(
                session_id,
                user_input,
                |_event| {},
                |_request| async move {
                    Ok(ApprovalDecision {
                        approved: false,
                        reason: Some(
                            "Mutating tools require an approval-capable client. Use the interactive cli."
                                .to_string(),
                        ),
                    })
                },
            )
            .await?;
        Ok(outcome)
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
        self.chat_with_approval_and_events(session_id, user_input, |_event| {}, approval)
            .await
    }

    pub async fn chat_with_approval_and_events<E, F, Fut>(
        &self,
        session_id: &str,
        user_input: &str,
        mut on_event: E,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        E: FnMut(&TurnEvent) + Send,
        F: Fn(ApprovalRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ApprovalDecision>> + Send,
    {
        let outcome = self
            .run_turn_with_approval(session_id, user_input, &mut on_event, approval)
            .await?;
        Ok(self.outcome_to_output(outcome))
    }

    pub async fn reset_session(&self, session_id: &str) -> Result<()> {
        self.state.remove_session(session_id).await;
        self.store.delete_session(session_id).await
    }

    pub async fn session_snapshot(&self, session_id: &str) -> Result<AgentSession> {
        if let Some(session) = self.state.session(session_id).await {
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
        let active_turn = self.state.active_turn(session_id).await;
        Ok(SessionSnapshot {
            session_id: session_id.to_string(),
            session_state: if active_turn.is_some() {
                SessionState::Busy
            } else {
                SessionState::Idle
            },
            active_turn: active_turn.as_ref().map(|turn| turn.turn_id.clone()),
            turn_state: active_turn.as_ref().map(|turn| turn.turn_state.clone()),
            message_count: session.messages.len(),
        })
    }

    pub async fn interrupt_session(&self, session_id: &str) -> bool {
        self.state.interrupt_session(session_id).await
    }

    pub(crate) async fn complete_model_request(
        &self,
        cancellation_token: &CancellationToken,
        request: ModelRequest,
    ) -> Result<ModelResponse> {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                bail!(TURN_INTERRUPTED_ERROR);
            }
            response = self.model.complete(request) => response,
        }
    }

    pub(crate) async fn await_approval<Fut>(
        &self,
        cancellation_token: &CancellationToken,
        approval_future: Fut,
    ) -> Result<ApprovalDecision>
    where
        Fut: std::future::Future<Output = Result<ApprovalDecision>> + Send,
    {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                bail!(TURN_INTERRUPTED_ERROR);
            }
            response = approval_future => response,
        }
    }

    pub(crate) async fn execute_tool_call(
        &self,
        cancellation_token: &CancellationToken,
        call: ToolCall,
        ctx: &agent_core::ToolExecutionContext,
    ) -> Result<agent_core::ToolResult> {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                bail!(TURN_INTERRUPTED_ERROR);
            }
            response = self.tools.execute(call, ctx) => response,
        }
    }

    async fn run_turn_with_approval<E, F, Fut>(
        &self,
        session_id: &str,
        user_input: &str,
        on_event: &mut E,
        approval: F,
    ) -> Result<TurnOutcome>
    where
        E: FnMut(&TurnEvent) + Send,
        F: Fn(ApprovalRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ApprovalDecision>> + Send,
    {
        let turn_id = next_turn_id();
        let active_turn = self
            .state
            .start_turn(session_id.to_string(), turn_id.clone())
            .await;

        let mut session = self.load_session(session_id).await?;
        session.push_user_message(user_input);

        let mut events = Vec::new();
        let session_for_interrupt = session.clone();
        emit_event(
            &mut events,
            on_event,
            TurnEvent::TurnStarted {
                turn_id: turn_id.clone(),
                session_id: session_id.to_string(),
                user_input: user_input.to_string(),
            },
        );
        let result = if active_turn.is_cancelled() {
            emit_event(
                &mut events,
                on_event,
                TurnEvent::TurnCancelled {
                    turn_id: turn_id.clone(),
                    reason: "interrupted by client".to_string(),
                },
            );
            Ok(TurnOutcome {
                turn_id: turn_id.clone(),
                final_response: "Turn cancelled.".to_string(),
                events,
                session,
                model_name: None,
                state: TurnState::Cancelled,
            })
        } else {
            RegularTurnTask
                .run(
                    TaskContext {
                        runtime: self,
                        session_id,
                        turn_id: &turn_id,
                        cancellation_token: active_turn.cancellation_token.clone(),
                        on_event,
                    },
                    session,
                    approval,
                )
                .await
        };

        self.state.finish_turn(session_id).await;

        match result {
            Ok(outcome) => {
                self.save_session(outcome.session.clone()).await?;
                Ok(outcome)
            }
            Err(err) => {
                if is_turn_interrupted_error(&err) {
                    self.save_session(session_for_interrupt.clone()).await?;
                    return Ok(TurnOutcome {
                        turn_id: turn_id.clone(),
                        final_response: "Turn cancelled.".to_string(),
                        events: vec![TurnEvent::TurnCancelled {
                            turn_id,
                            reason: "interrupted by client".to_string(),
                        }],
                        session: session_for_interrupt,
                        model_name: None,
                        state: TurnState::Cancelled,
                    });
                }
                let mut session = self.load_session(session_id).await?;
                session.push_assistant_message(Some(format!("Turn failed: {err:#}")), Vec::new());
                self.save_session(session.clone()).await?;
                let error_text = format!("{err:#}");
                Ok(TurnOutcome {
                    turn_id: turn_id.clone(),
                    final_response: format!("Turn failed: {error_text}"),
                    events: vec![TurnEvent::TurnFailed {
                        turn_id,
                        error: error_text,
                    }],
                    session,
                    model_name: None,
                    state: TurnState::Failed,
                })
            }
        }
    }

    async fn load_session(&self, session_id: &str) -> Result<AgentSession> {
        if let Some(session) = self.state.session(session_id).await {
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
        self.state.save_session(session.clone()).await;
        Ok(session)
    }

    async fn save_session(&self, session: AgentSession) -> Result<()> {
        self.store.save_session(&session).await?;
        self.state.save_session(session).await;
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

fn is_turn_interrupted_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string() == TURN_INTERRUPTED_ERROR)
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
            tools: request.tools.iter().map(ChatToolSpec::from_spec).collect(),
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
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => Ok(Self {
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

fn next_turn_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("turn-{now}")
}

pub(crate) fn emit_event<E>(events: &mut Vec<TurnEvent>, on_event: &mut E, event: TurnEvent)
where
    E: FnMut(&TurnEvent),
{
    events.push(event.clone());
    on_event(&event);
}

impl AgentRuntime {
    pub(crate) async fn is_turn_cancelled(&self, session_id: &str) -> bool {
        self.state
            .active_turn(session_id)
            .await
            .is_some_and(|turn| turn.is_cancelled())
    }
}

pub(crate) fn summarize_arguments(arguments: &Value) -> String {
    let rendered =
        serde_json::to_string(arguments).unwrap_or_else(|_| "<invalid-json>".to_string());
    if rendered.chars().count() > 240 {
        let truncated = rendered.chars().take(240).collect::<String>();
        format!("{truncated}...")
    } else {
        rendered
    }
}
