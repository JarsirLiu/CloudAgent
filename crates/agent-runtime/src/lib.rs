mod state;
mod tasks;

use agent_core::{
    AgentContext, AgentTurnOutput, ChatModel, ConversationHistory, ConversationState,
    ExecutionPolicy, ModelRequest, ModelResponse, RolloutItem, ToolCall, ToolExecutor, ToolSpec,
    agent_turn_output_from_events, transcript_items_from_rollout_items,
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
use storage::JsonConversationStore;
use tasks::{RegularTurnTask, RuntimeTask, TaskContext, TurnOutcome};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub use agent_core::ResponseItem;
pub use agent_protocol::{
    ConversationSnapshot, ConversationStatus, EventMsg, RequestId, ServerRequest,
    ServerRequestDecision, TranscriptItem, TurnItemKind, TurnState,
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
    store: JsonConversationStore,
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
        let store = JsonConversationStore::new(config.runtime.conversation_store_dir.clone());

        let system_prompt = config.runtime.system_prompt.clone();

        Ok(Self {
            config,
            context,
            policy,
            model,
            tools,
            state: RuntimeState::new(system_prompt),
            store,
        })
    }

    pub async fn chat(&self, conversation_id: &str, user_input: &str) -> Result<AgentTurnOutput> {
        let outcome = self
            .chat_with_approval_and_events(
                conversation_id,
                user_input,
                |_event| {},
                |_request| async move {
                    Ok(ServerRequestDecision {
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
        conversation_id: &str,
        user_input: &str,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        self.chat_with_approval_and_events(conversation_id, user_input, |_event| {}, approval)
            .await
    }

    pub async fn chat_with_approval_and_events<E, F, Fut>(
        &self,
        conversation_id: &str,
        user_input: &str,
        mut on_event: E,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        E: FnMut(&EventMsg) + Send,
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        let outcome = self
            .run_turn_with_approval(conversation_id, user_input, &mut on_event, approval)
            .await?;
        Ok(self.outcome_to_output(outcome))
    }

    pub async fn reset_conversation(&self, conversation_id: &str) -> Result<()> {
        self.state.remove_conversation(conversation_id).await;
        self.store.delete_conversation(conversation_id).await?;
        self.store.delete_events(conversation_id).await
    }

    pub async fn conversation_history_snapshot(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationHistory> {
        Ok(self
            .conversation_snapshot(conversation_id)
            .await?
            .history()
            .clone())
    }

    pub async fn conversation_transcript_snapshot(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<TranscriptItem>> {
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        Ok(transcript_items_from_rollout_items(&rollout_items))
    }

    pub async fn conversation_snapshot(&self, conversation_id: &str) -> Result<ConversationState> {
        if let Some(conversation) = self.state.conversation(conversation_id).await {
            return Ok(conversation);
        }
        if let Some(mut conversation) = self.store.load_conversation(conversation_id).await? {
            conversation
                .context_mut()
                .ensure_system_prompt(self.config.runtime.system_prompt.clone());
            return Ok(conversation);
        }
        Ok(ConversationState::new(ConversationHistory::new(
            conversation_id.to_string(),
            self.config.runtime.system_prompt.clone(),
        )))
    }

    pub fn default_conversation_id(&self) -> &str {
        &self.config.runtime.default_conversation_id
    }

    pub async fn conversation_status(&self, conversation_id: &str) -> Result<ConversationSnapshot> {
        let history = self.conversation_history_snapshot(conversation_id).await?;
        let active_turn = self.state.active_turn(conversation_id).await;
        Ok(ConversationSnapshot {
            conversation_id: conversation_id.to_string(),
            conversation_status: if active_turn.is_some() {
                ConversationStatus::Busy
            } else {
                ConversationStatus::Idle
            },
            active_turn: active_turn.as_ref().map(|turn| turn.turn_id.clone()),
            turn_state: active_turn.as_ref().map(|turn| turn.turn_state.clone()),
            message_count: history.messages.len(),
        })
    }

    pub async fn interrupt_conversation(&self, conversation_id: &str) -> bool {
        self.state.interrupt_conversation(conversation_id).await
    }

    pub async fn register_pending_request(
        &self,
        conversation_id: &str,
        request_id: RequestId,
        request: ServerRequest,
    ) {
        self.state
            .set_pending_request(conversation_id, request_id, request)
            .await;
    }

    pub async fn resolve_pending_request(&self, conversation_id: &str, request_id: &RequestId) {
        self.state
            .resolve_pending_request(conversation_id, request_id)
            .await;
    }

    pub(crate) async fn complete_model_request_streaming(
        &self,
        cancellation_token: &CancellationToken,
        request: ModelRequest,
        on_text_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ModelResponse> {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                bail!(TURN_INTERRUPTED_ERROR);
            }
            response = self.model.complete_streaming(request, on_text_delta) => response,
        }
    }

    pub(crate) async fn await_approval<Fut>(
        &self,
        cancellation_token: &CancellationToken,
        approval_future: Fut,
    ) -> Result<ServerRequestDecision>
    where
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
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
        conversation_id: &str,
        user_input: &str,
        on_event: &mut E,
        approval: F,
    ) -> Result<TurnOutcome>
    where
        E: FnMut(&EventMsg) + Send,
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        let (persist_tx, mut persist_rx) = mpsc::unbounded_channel::<EventMsg>();
        let store = self.store.clone();
        let persisted_conversation_id = conversation_id.to_string();
        let persist_task = tokio::spawn(async move {
            while let Some(event) = persist_rx.recv().await {
                store
                    .append_event(&persisted_conversation_id, &event)
                    .await?;
            }
            Result::<()>::Ok(())
        });
        let mut event_sink = |event: &EventMsg| {
            self.state
                .append_conversation_event(conversation_id, event.clone());
            let _ = persist_tx.send(event.clone());
            on_event(event);
        };

        let turn_id = next_turn_id();
        let active_turn = self
            .state
            .start_turn(conversation_id.to_string(), turn_id.clone())
            .await;

        let mut history = self.load_history(conversation_id).await?;
        let user_item = history.push_user_message(user_input);
        self.state.save_history(history.clone()).await;
        self.append_response_item_to_rollout(conversation_id, user_item)
            .await?;

        let mut events = Vec::new();
        let history_for_interrupt = history.clone();
        emit_event(
            &mut events,
            &mut event_sink,
            EventMsg::TurnStarted {
                turn_id: turn_id.clone(),
                conversation_id: conversation_id.to_string(),
                user_input: user_input.to_string(),
            },
        );
        let result = if active_turn.is_cancelled() {
            emit_event(
                &mut events,
                &mut event_sink,
                EventMsg::TurnCancelled {
                    turn_id: turn_id.clone(),
                    reason: "interrupted by client".to_string(),
                },
            );
            Ok(TurnOutcome {
                turn_id: turn_id.clone(),
                final_response: "Turn cancelled.".to_string(),
                events,
                history,
                model_name: None,
                state: TurnState::Cancelled,
            })
        } else {
            RegularTurnTask
                .run(
                    TaskContext {
                        runtime: self,
                        conversation_id,
                        turn_id: &turn_id,
                        cancellation_token: active_turn.cancellation_token.clone(),
                        on_event: &mut event_sink,
                    },
                    history,
                    approval,
                )
                .await
        };

        self.state.finish_turn(conversation_id).await;

        match result {
            Ok(outcome) => {
                drop(event_sink);
                drop(persist_tx);
                self.save_history(outcome.history.clone()).await?;
                persist_task.await??;
                Ok(outcome)
            }
            Err(err) => {
                if is_turn_interrupted_error(&err) {
                    let mut events = Vec::new();
                    emit_event(
                        &mut events,
                        &mut event_sink,
                        EventMsg::TurnCancelled {
                            turn_id: turn_id.clone(),
                            reason: "interrupted by client".to_string(),
                        },
                    );
                    drop(event_sink);
                    drop(persist_tx);
                    self.save_history(history_for_interrupt.clone()).await?;
                    persist_task.await??;
                    let outcome = TurnOutcome {
                        turn_id: turn_id.clone(),
                        final_response: "Turn cancelled.".to_string(),
                        events,
                        history: history_for_interrupt,
                        model_name: None,
                        state: TurnState::Cancelled,
                    };
                    return Ok(outcome);
                }
                let mut history = self.load_history(conversation_id).await?;
                let failed_item = history
                    .push_assistant_message(Some(format!("Turn failed: {err:#}")), Vec::new());
                self.append_response_item_to_rollout(conversation_id, failed_item)
                    .await?;
                let error_text = format!("{err:#}");
                let mut events = Vec::new();
                emit_event(
                    &mut events,
                    &mut event_sink,
                    EventMsg::TurnFailed {
                        turn_id: turn_id.clone(),
                        error: error_text.clone(),
                    },
                );
                drop(event_sink);
                drop(persist_tx);
                self.save_history(history.clone()).await?;
                persist_task.await??;
                let outcome = TurnOutcome {
                    turn_id: turn_id.clone(),
                    final_response: format!("Turn failed: {error_text}"),
                    events,
                    history,
                    model_name: None,
                    state: TurnState::Failed,
                };
                Ok(outcome)
            }
        }
    }

    async fn load_history(&self, conversation_id: &str) -> Result<ConversationHistory> {
        if let Some(history) = self.state.history(conversation_id).await {
            return Ok(history);
        }

        let mut conversation =
            if let Some(conversation) = self.store.load_conversation(conversation_id).await? {
                conversation
            } else {
                ConversationState::new(ConversationHistory::new(
                    conversation_id.to_string(),
                    self.config.runtime.system_prompt.clone(),
                ))
            };
        conversation
            .context_mut()
            .ensure_system_prompt(self.config.runtime.system_prompt.clone());
        let history = conversation.history().clone();
        self.state.save_conversation(conversation).await;
        Ok(history)
    }

    async fn save_history(&self, history: ConversationHistory) -> Result<()> {
        let conversation_id = history.id.clone();
        self.state.save_history(history).await;
        if let Some(conversation) = self.state.conversation(&conversation_id).await {
            self.store.save_conversation(&conversation).await?;
        }
        Ok(())
    }

    pub(crate) async fn append_response_item_to_rollout(
        &self,
        conversation_id: &str,
        item: ResponseItem,
    ) -> Result<()> {
        self.store
            .append_rollout_items(conversation_id, &[RolloutItem::from(item)])
            .await
    }

    fn outcome_to_output(&self, outcome: TurnOutcome) -> AgentTurnOutput {
        agent_turn_output_from_events(
            outcome.turn_id,
            outcome.final_response,
            outcome.events,
            &outcome.history,
            outcome.model_name,
            outcome.state,
        )
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
            stream: None,
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

    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_text_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ModelResponse> {
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
            stream: Some(true),
        };

        let mut response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await
            .context("failed to send streaming LLM request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .context("failed to read streaming LLM error body")?;
            bail!("LLM streaming request failed with status {status}: {body}");
        }

        let mut content = String::new();
        let mut model_name: Option<String> = None;
        let mut stream_buffer = String::new();
        let mut tool_calls_acc: std::collections::HashMap<usize, StreamingToolCallAcc> =
            std::collections::HashMap::new();

        while let Some(chunk) = response
            .chunk()
            .await
            .context("failed reading streaming response chunk")?
        {
            stream_buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = stream_buffer.find('\n') {
                let line = stream_buffer[..pos].trim().to_string();
                stream_buffer = stream_buffer[pos + 1..].to_string();
                if line.is_empty() || !line.starts_with("data:") {
                    continue;
                }
                let data = line.trim_start_matches("data:").trim();
                if data == "[DONE]" {
                    break;
                }
                let parsed: ChatCompletionStreamChunk = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if model_name.is_none() {
                    model_name = Some(parsed.model.clone());
                }
                for choice in parsed.choices {
                    if let Some(delta) = choice.delta.content
                        && !delta.is_empty()
                    {
                        on_text_delta(delta.clone());
                        content.push_str(&delta);
                    }
                    if let Some(delta_tool_calls) = choice.delta.tool_calls {
                        for delta_call in delta_tool_calls {
                            let index = delta_call.index;
                            let acc = tool_calls_acc.entry(index).or_default();
                            if let Some(id) = delta_call.id {
                                acc.id = id;
                            }
                            if let Some(function) = delta_call.function {
                                if let Some(name) = function.name {
                                    acc.name = name;
                                }
                                if let Some(arguments) = function.arguments {
                                    acc.arguments.push_str(&arguments);
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut tool_calls = Vec::new();
        let mut ordered: Vec<(usize, StreamingToolCallAcc)> = tool_calls_acc.into_iter().collect();
        ordered.sort_by_key(|(idx, _)| *idx);
        for (_, acc) in ordered {
            if acc.id.is_empty() || acc.name.is_empty() {
                continue;
            }
            let arguments = serde_json::from_str::<Value>(&acc.arguments)
                .unwrap_or_else(|_| Value::String(acc.arguments.clone()));
            tool_calls.push(ToolCall {
                id: acc.id,
                name: acc.name,
                arguments,
            });
        }

        Ok(ModelResponse {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            tool_calls,
            model_name,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
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
    fn from_message(message: &ResponseItem) -> Result<Self> {
        match message {
            ResponseItem::System { content } => Ok(Self {
                role: "system".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }),
            ResponseItem::User { content } => Ok(Self {
                role: "user".to_string(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }),
            ResponseItem::Assistant {
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
            ResponseItem::Tool {
                tool_call_id,
                name,
                content,
                ..
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

#[derive(Default)]
struct StreamingToolCallAcc {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatCompletionStreamChunk {
    model: String,
    choices: Vec<ChatCompletionStreamChoice>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamChoice {
    delta: ChatCompletionStreamDelta,
}

#[derive(Deserialize)]
struct ChatCompletionStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatCompletionStreamToolCallDelta>>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ChatCompletionStreamFunctionDelta>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

fn next_turn_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("turn-{now}")
}

pub(crate) fn emit_event<E>(events: &mut Vec<EventMsg>, on_event: &mut E, event: EventMsg)
where
    E: FnMut(&EventMsg),
{
    events.push(event.clone());
    on_event(&event);
}

impl AgentRuntime {
    pub(crate) async fn is_turn_cancelled(&self, conversation_id: &str) -> bool {
        self.state
            .active_turn(conversation_id)
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
