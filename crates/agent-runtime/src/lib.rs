mod engine;
mod state;
mod tasks;
mod tools;

use agent_core::{
    AgentContext, AgentTurnOutput, ChatModel, ConversationHistory, ConversationState,
    ConversationTurn, EnvironmentContext, ExecutionPolicy, ModelRequest, ModelResponse, ToolCall,
    ToolExecutor, agent_turn_output_from_events, build_turns_from_rollout_items,
    flatten_conversation_turns,
};
use agent_tools::ToolRegistry;
use anyhow::{Result, bail};
use config::AgentConfig;
use engine::{
    OpenAiCompatibleModel, approve_tool_for_session, emit_event, is_tool_approved_for_session,
    is_turn_interrupted_error, model_shell_name, next_turn_id, run_turn_with_approval,
    summarize_arguments, visible_message_count,
};
use state::RuntimeState;
use state::rollout_recorder::RolloutRecorder;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use storage::JsonConversationStore;
use tasks::TurnOutcome;
use tokio_util::sync::CancellationToken;

pub use agent_core::ResponseItem;
pub use agent_protocol::{
    ConversationSnapshot, ConversationStatus, ConversationSummary, EventMsg, RequestId,
    ServerRequest, ServerRequestDecision, TranscriptItem, TurnItemKind, TurnState,
};

const TURN_INTERRUPTED_ERROR: &str = "turn interrupted by client";
const MANUAL_COMPACTION_MIN_HISTORY_TOKENS: usize = 20_000;

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
    rollout_recorder: RolloutRecorder,
    session_approvals: StdMutex<HashSet<String>>,
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
        let rollout_recorder = RolloutRecorder::new(store.clone());

        let system_prompt = config.runtime.system_prompt.clone();

        Ok(Self {
            config,
            context,
            policy,
            model,
            tools,
            state: RuntimeState::new(system_prompt),
            store,
            rollout_recorder,
            session_approvals: StdMutex::new(HashSet::new()),
        })
    }

    pub async fn chat(&self, conversation_id: &str, user_input: &str) -> Result<AgentTurnOutput> {
        let outcome = self
            .chat_with_approval_and_events(
                conversation_id,
                user_input,
                |_event| {},
                |_request| async move {
                    Ok(ServerRequestDecision::decline(Some(
                            "Mutating tools require an approval-capable client. Use the interactive cli."
                                .to_string(),
                        )))
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
        let outcome =
            run_turn_with_approval(self, conversation_id, user_input, &mut on_event, approval)
                .await?;
        Ok(self.outcome_to_output(outcome))
    }

    pub async fn reset_conversation(&self, conversation_id: &str) -> Result<()> {
        self.rollout_recorder.flush().await?;
        self.state.remove_conversation(conversation_id).await;
        self.store.delete_conversation(conversation_id).await?;
        self.store.delete_events(conversation_id).await
    }

    pub async fn create_conversation(&self, conversation_id: &str) -> Result<()> {
        self.store.create_conversation(conversation_id).await
    }

    pub async fn archive_conversation(&self, conversation_id: &str) -> Result<()> {
        self.rollout_recorder.flush().await?;
        self.state.remove_conversation(conversation_id).await;
        self.store.archive_conversation(conversation_id).await
    }

    pub async fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        Ok(self
            .store
            .list_conversations()
            .await?
            .into_iter()
            .map(|summary| ConversationSummary {
                conversation_id: summary.conversation_id,
                message_count: summary.message_count,
                updated_at_ms: summary.updated_at_ms,
            })
            .collect())
    }

    pub async fn compact_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ManualCompactionOutcome> {
        self.rollout_recorder.flush().await?;
        tasks::run_manual_compaction(self, conversation_id, MANUAL_COMPACTION_MIN_HISTORY_TOKENS)
            .await
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
        Ok(flatten_conversation_turns(
            &self.build_turns_from_rollout(conversation_id).await?,
        ))
    }

    pub async fn build_turns_from_rollout(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<ConversationTurn>> {
        self.rollout_recorder.flush().await?;
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        Ok(build_turns_from_rollout_items(&rollout_items))
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

    pub(crate) fn environment_context(&self) -> EnvironmentContext {
        let now = chrono::Local::now();
        EnvironmentContext::new(
            self.context.workspace_root.clone(),
            model_shell_name(),
            now.format("%Y-%m-%d").to_string(),
            now.format("%H:%M:%S").to_string(),
            now.to_rfc3339(),
            now.offset().to_string(),
        )
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
            message_count: visible_message_count(&history),
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

    pub(crate) async fn execute_tool_call_streaming<F>(
        &self,
        cancellation_token: &CancellationToken,
        call: ToolCall,
        ctx: &agent_core::ToolExecutionContext,
        mut on_output_delta: F,
    ) -> Result<agent_core::ToolResult>
    where
        F: FnMut(agent_core::ToolOutputDelta) + Send,
    {
        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel();
        let streaming_ctx = ctx.clone().with_output_tx(output_tx);
        let execution = self.tools.execute(call, &streaming_ctx);
        tokio::pin!(execution);

        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    bail!(TURN_INTERRUPTED_ERROR);
                }
                Some(delta) = output_rx.recv() => {
                    on_output_delta(delta);
                }
                response = &mut execution => {
                    while let Ok(delta) = output_rx.try_recv() {
                        on_output_delta(delta);
                    }
                    return response;
                }
            }
        }
    }

    pub(crate) fn is_tool_approved_for_session(&self, call: &ToolCall) -> bool {
        is_tool_approved_for_session(self, call)
    }

    pub(crate) fn approve_tool_for_session(&self, call: &ToolCall) {
        approve_tool_for_session(self, call);
    }

    fn outcome_to_output(&self, outcome: TurnOutcome) -> AgentTurnOutput {
        agent_turn_output_from_events(
            outcome.turn_id,
            outcome.events,
            &outcome.history,
            outcome.model_name,
            outcome.state,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn visible_message_count_excludes_system_and_tool_items() {
        let mut history = ConversationHistory::new("default", "system");
        history.push_user_message("hello");
        history.push_assistant_message(
            None,
            vec![ToolCall {
                id: "call-1".to_string(),
                name: "shell_command".to_string(),
                arguments: json!({"command": "pwd"}),
            }],
        );
        history.push_tool_result(agent_core::ToolResult {
            tool_call_id: "call-1".to_string(),
            name: "shell_command".to_string(),
            content: "D:\\work".to_string(),
            is_error: false,
            structured: None,
        });
        history.push_assistant_message(Some("done".to_string()), Vec::new());

        assert_eq!(visible_message_count(&history), 2);
    }
}

impl AgentRuntime {
    pub(crate) async fn is_turn_cancelled(&self, conversation_id: &str) -> bool {
        self.state
            .active_turn(conversation_id)
            .await
            .is_some_and(|turn| turn.is_cancelled())
    }
}

#[derive(Debug, Clone)]
pub enum ManualCompactionOutcome {
    Compacted {
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    Skipped {
        estimated_history_tokens: usize,
    },
}
