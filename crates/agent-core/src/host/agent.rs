use super::{
    AgentHostExt, AgentHostParts, AgentMetadata, ConversationStoreBackend, MemoryBackend,
    RolloutRecorderBackend,
};
use crate::context::EnvironmentContext;
use crate::conversation::{
    ConversationHistory, ConversationSnapshot, ConversationState, ConversationStatus,
    ConversationSummary,
};
use crate::observability::{AuditEventEntry, append_audit_event_safe, verify_audit_chain};
use crate::projection::conversation_history_from_rollout_items;
use crate::projection::flatten_conversation_turns;
use crate::rollout::RolloutItem;
use crate::tool::{ToolBackend, ToolCall, ToolResult, ToolSpec, summarize_arguments};
use crate::turn::{
    AgentTurnOutput, EventMsg, ManualCompactionOutcome, RequestId, RestoredBudgetBaseline,
    ServerRequest, ServerRequestDecision, ServerRequestHandler, TurnHost, chat as core_chat,
    chat_with_approval as core_chat_with_approval,
    chat_with_approval_and_events as core_chat_with_approval_and_events,
    compact_conversation as core_compact_conversation,
};
use crate::{
    ActiveTurnHandle, AgentContext, AgentState, ApprovalGrantStoreBackend, ApprovalPolicy,
    ChatModel, ContextManager, ConversationTurn, ExecutionPolicy, PermissionProfile,
    RegularTurnSettings, ResponseItem, build_turns_from_rollout_items, complete_model_request,
    complete_model_request_streaming, input_items_attachment_count, input_items_preview_text,
    paginate_turns, visible_message_count,
};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const TURN_INTERRUPTED_ERROR: &str = "turn interrupted by client";
pub const MANUAL_COMPACTION_MIN_HISTORY_TOKENS: usize = 20_000;

pub struct AgentHost {
    metadata: AgentMetadata,
    context: AgentContext,
    regular_turn_settings: RegularTurnSettings,
    policy: ExecutionPolicy,
    model: Arc<dyn ChatModel>,
    tools: Arc<
        dyn ToolBackend<PermissionProfile = PermissionProfile, ApprovalPolicy = ApprovalPolicy>,
    >,
    state: AgentState,
    store: Arc<dyn ConversationStoreBackend>,
    approval_grants: Arc<dyn ApprovalGrantStoreBackend>,
    rollout_recorder: Arc<dyn RolloutRecorderBackend>,
    memory: Arc<dyn MemoryBackend>,
}

impl AgentHost {
    pub fn new(parts: AgentHostParts) -> Self {
        Self {
            metadata: parts.metadata,
            context: parts.context,
            regular_turn_settings: parts.regular_turn_settings,
            policy: parts.policy,
            model: parts.model,
            tools: parts.tools,
            state: parts.state,
            store: parts.store,
            approval_grants: parts.approval_grants,
            rollout_recorder: parts.rollout_recorder,
            memory: parts.memory,
        }
    }

    pub async fn run_startup_retention_cleanup(&self) {
        let _ = self.store.prune_archived_conversations_if_needed().await;
        if let Err(err) = verify_audit_chain(&self.context.workspace_root) {
            tracing::warn!("audit chain verify failed on startup: {err:#}");
        }
    }

    pub fn llm_model_name(&self) -> &str {
        &self.metadata.llm_model_name
    }

    pub fn conversation_store_dir(&self) -> &Path {
        &self.metadata.conversation_store_dir
    }

    pub fn cli_pre_llm_filter_enabled(&self) -> bool {
        self.metadata.cli_pre_llm_filter_enabled
    }

    pub fn cli_permission_mode(&self) -> &str {
        &self.metadata.cli_permission_mode
    }

    pub fn new_conversation_id(&self) -> String {
        Uuid::now_v7().to_string()
    }

    pub async fn create_draft_conversation(&self) -> Result<String> {
        let id = self.new_conversation_id();
        self.store.mark_active_conversation(&id).await?;
        Ok(id)
    }

    pub async fn ensure_active_conversation(&self) -> Result<String> {
        if let Some(id) = self.store.load_active_conversation().await?
            && !id.trim().is_empty()
        {
            return Ok(id);
        }
        self.create_draft_conversation().await
    }

    pub async fn mark_active_conversation(&self, conversation_id: &str) -> Result<()> {
        self.store.mark_active_conversation(conversation_id).await
    }

    pub async fn load_active_conversation(&self) -> Result<Option<String>> {
        self.store.load_active_conversation().await
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

    pub async fn has_conversation(&self, conversation_id: &str) -> Result<bool> {
        self.store.has_conversation(conversation_id).await
    }

    pub async fn ensure_conversation_persisted(&self, conversation_id: &str) -> Result<bool> {
        if self.has_conversation(conversation_id).await? {
            return Ok(false);
        }
        self.create_conversation(conversation_id).await?;
        Ok(true)
    }

    pub async fn archive_conversation(&self, conversation_id: &str) -> Result<()> {
        self.rollout_recorder.flush().await?;
        self.state.remove_conversation(conversation_id).await;
        self.store.archive_conversation(conversation_id).await
    }

    pub async fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        self.store.list_conversations().await
    }

    pub async fn set_conversation_title(&self, conversation_id: &str, title: &str) -> Result<()> {
        self.store
            .set_conversation_title(conversation_id, title)
            .await
    }

    pub async fn suggest_conversation_title(
        &self,
        user_input: &[crate::InputItem],
    ) -> Result<String> {
        let request = crate::ModelRequest {
            messages: vec![
                ResponseItem::System {
                    content:
                        "Generate a short session title (max 8 words). Return title text only."
                            .to_string(),
                },
                ResponseItem::User {
                    content: user_input.to_vec(),
                },
            ],
            tools: Vec::new(),
            temperature: 0.2,
        };
        let response = self.model.complete(request).await?;
        Ok(response
            .content
            .unwrap_or_default()
            .trim()
            .trim_matches('"')
            .to_string())
    }

    pub async fn conversation_history_snapshot(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationHistory> {
        self.history_from_rollout(conversation_id).await
    }

    pub async fn conversation_transcript_snapshot(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<crate::TranscriptItem>> {
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

    pub async fn build_turns_page_from_rollout(
        &self,
        conversation_id: &str,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<ConversationTurn>, bool, Option<String>)> {
        let turns = self.build_turns_from_rollout(conversation_id).await?;
        Ok(paginate_turns(turns, before_turn_id, limit))
    }

    pub async fn conversation_snapshot(&self, conversation_id: &str) -> Result<ConversationState> {
        if let Some(conversation) = self.state.conversation(conversation_id).await {
            return Ok(conversation);
        }
        let history = self.history_from_rollout(conversation_id).await?;
        Ok(ConversationState::new(history))
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

    pub async fn compact_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ManualCompactionOutcome> {
        self.compact_conversation_with_minimum(
            conversation_id,
            MANUAL_COMPACTION_MIN_HISTORY_TOKENS,
        )
        .await
    }

    pub async fn compact_conversation_with_minimum(
        &self,
        conversation_id: &str,
        minimum_history_tokens: usize,
    ) -> Result<ManualCompactionOutcome> {
        core_compact_conversation(self, conversation_id, minimum_history_tokens).await
    }

    pub async fn chat(
        &self,
        conversation_id: &str,
        user_input: &[crate::InputItem],
    ) -> Result<AgentTurnOutput> {
        core_chat(
            self,
            conversation_id,
            user_input,
            &PermissionProfile::ReadOnly,
            &ApprovalPolicy::OnRequest,
        )
        .await
    }

    pub async fn chat_with_approval<F, Fut>(
        &self,
        conversation_id: &str,
        user_input: &[crate::InputItem],
        permission_profile: &PermissionProfile,
        approval_policy: &ApprovalPolicy,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        core_chat_with_approval(
            self,
            conversation_id,
            user_input,
            permission_profile,
            approval_policy,
            approval,
        )
        .await
    }

    pub async fn chat_with_approval_and_events<E, F, Fut>(
        &self,
        conversation_id: &str,
        user_input: &[crate::InputItem],
        permission_profile: &PermissionProfile,
        approval_policy: &ApprovalPolicy,
        mut on_event: E,
        approval: F,
    ) -> Result<AgentTurnOutput>
    where
        E: FnMut(&EventMsg) + Send,
        F: Fn(ServerRequest) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
    {
        core_chat_with_approval_and_events(
            self,
            conversation_id,
            user_input,
            permission_profile,
            approval_policy,
            &mut on_event,
            approval,
        )
        .await
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

    fn append_audit(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        event_type: &str,
        severity: &str,
        payload: serde_json::Value,
    ) {
        let payload_json = serde_json::to_string(&payload)
            .unwrap_or_else(|_| "{\"error\":\"payload_serialize_failed\"}".to_string());
        let entry = AuditEventEntry {
            session_id,
            turn_id,
            event_type,
            severity,
            payload_json,
        };
        append_audit_event_safe(&self.context.workspace_root, &entry);
    }

    pub(crate) fn audit_turn_tool_started(&self, session_id: &str, turn_id: &str, call: &ToolCall) {
        self.append_audit(
            session_id,
            Some(turn_id),
            "tool.started",
            "info",
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "arguments_preview": summarize_arguments(&call.arguments)
            }),
        );
    }

    pub(crate) fn audit_turn_tool_completed(
        &self,
        session_id: &str,
        turn_id: &str,
        call: &ToolCall,
        result: &ToolResult,
    ) {
        self.append_audit(
            session_id,
            Some(turn_id),
            if result.is_error {
                "tool.failed"
            } else {
                "tool.completed"
            },
            if result.is_error { "error" } else { "info" },
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "is_error": result.is_error,
                "content_preview": result.content.chars().take(400).collect::<String>()
            }),
        );
    }

    pub(crate) fn audit_tool_approval_requested(
        &self,
        session_id: &str,
        turn_id: &str,
        call: &ToolCall,
        reason: String,
    ) {
        self.append_audit(
            session_id,
            Some(turn_id),
            "approval.requested",
            "info",
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "reason": reason,
                "arguments_preview": summarize_arguments(&call.arguments)
            }),
        );
    }

    pub(crate) fn audit_tool_approval_decided(
        &self,
        session_id: &str,
        turn_id: &str,
        call: &ToolCall,
        decision: &ServerRequestDecision,
    ) {
        self.append_audit(
            session_id,
            Some(turn_id),
            "approval.decided",
            "info",
            json!({
                "tool_call_id": call.id,
                "tool_name": call.name,
                "decision": format!("{:?}", decision.decision),
                "reason": decision.reason
            }),
        );
    }

    pub(crate) fn environment_context(&self) -> EnvironmentContext {
        let now = chrono::Local::now();
        EnvironmentContext::new(
            self.context.workspace_root.clone(),
            self.metadata.shell_name.clone(),
            now.format("%Y-%m-%d").to_string(),
            now.format("%H:%M:%S").to_string(),
            now.to_rfc3339(),
            now.offset().to_string(),
        )
    }

    async fn load_history(&self, conversation_id: &str) -> Result<ConversationHistory> {
        if let Some(history) = self.state.history(conversation_id).await
            && !is_placeholder_history(&history)
        {
            return Ok(history);
        }

        self.rollout_recorder.flush().await?;
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        if !rollout_items.is_empty() {
            let history = conversation_history_from_rollout_items(
                conversation_id.to_string(),
                self.metadata.system_prompt.clone(),
                &rollout_items,
            );
            self.save_history(history.clone()).await?;
            return Ok(history);
        }

        let history = ConversationHistory::new(
            conversation_id.to_string(),
            self.metadata.system_prompt.clone(),
        );
        self.save_history(history.clone()).await?;
        Ok(history)
    }

    async fn history_from_rollout(&self, conversation_id: &str) -> Result<ConversationHistory> {
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        Ok(conversation_history_from_rollout_items(
            conversation_id.to_string(),
            self.metadata.system_prompt.clone(),
            &rollout_items,
        ))
    }

    async fn save_history(&self, history: ConversationHistory) -> Result<()> {
        self.state.save_history(history).await;
        Ok(())
    }

    async fn persist_rollout_items(
        &self,
        conversation_id: &str,
        items: &[RolloutItem],
    ) -> Result<()> {
        self.record_rollout_items(conversation_id, items)
    }

    fn record_rollout_items(&self, conversation_id: &str, items: &[RolloutItem]) -> Result<()> {
        self.rollout_recorder.record_items(conversation_id, items)
    }

    async fn is_turn_cancelled(&self, conversation_id: &str) -> bool {
        self.state
            .active_turn(conversation_id)
            .await
            .is_some_and(|turn| turn.is_cancelled())
    }
}

impl AgentHostExt for AgentHost {
    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }
    fn context(&self) -> &AgentContext {
        &self.context
    }
    fn regular_turn_settings(&self) -> &RegularTurnSettings {
        &self.regular_turn_settings
    }
    fn policy(&self) -> &ExecutionPolicy {
        &self.policy
    }
    fn model(&self) -> &Arc<dyn ChatModel> {
        &self.model
    }
    fn tools(
        &self,
    ) -> &Arc<dyn ToolBackend<PermissionProfile = PermissionProfile, ApprovalPolicy = ApprovalPolicy>>
    {
        &self.tools
    }
    fn state(&self) -> &AgentState {
        &self.state
    }
    fn store(&self) -> &Arc<dyn ConversationStoreBackend> {
        &self.store
    }
    fn approval_grants(&self) -> &Arc<dyn ApprovalGrantStoreBackend> {
        &self.approval_grants
    }
    fn rollout_recorder(&self) -> &Arc<dyn RolloutRecorderBackend> {
        &self.rollout_recorder
    }
    fn memory(&self) -> &Arc<dyn MemoryBackend> {
        &self.memory
    }
}

#[async_trait]
impl TurnHost for AgentHost {
    type PermissionProfile = PermissionProfile;
    type ApprovalPolicy = ApprovalPolicy;

    fn turn_interrupted_error(&self) -> &'static str {
        TURN_INTERRUPTED_ERROR
    }
    fn regular_turn_settings(&self) -> RegularTurnSettings {
        self.regular_turn_settings.clone()
    }
    fn environment_context(&self) -> EnvironmentContext {
        self.environment_context()
    }
    fn raw_memory_fragment(&self) -> Option<String> {
        self.memory.raw_memory_fragment()
    }
    fn resolve_regular_turn_tool_exposure(
        &self,
        permission_profile: &PermissionProfile,
    ) -> crate::RegularTurnToolExposure {
        self.tools
            .resolve_regular_turn_tool_exposure(permission_profile)
    }

    async fn start_turn(
        &self,
        conversation_id: String,
        turn_id: String,
    ) -> Option<ActiveTurnHandle> {
        self.state.start_turn(conversation_id, turn_id).await
    }
    async fn finish_turn(&self, conversation_id: &str) {
        self.state.finish_turn(conversation_id).await;
    }
    async fn is_turn_cancelled(&self, conversation_id: &str) -> bool {
        self.is_turn_cancelled(conversation_id).await
    }
    fn append_conversation_event(&self, conversation_id: &str, event: EventMsg) {
        self.state.append_conversation_event(conversation_id, event);
    }
    async fn load_history(&self, conversation_id: &str) -> Result<ConversationHistory> {
        self.load_history(conversation_id).await
    }
    async fn history_from_rollout(&self, conversation_id: &str) -> Result<ConversationHistory> {
        self.history_from_rollout(conversation_id).await
    }
    async fn restore_budget_baseline(
        &self,
        conversation_id: &str,
    ) -> Result<Option<RestoredBudgetBaseline>> {
        self.rollout_recorder.flush().await?;
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        Ok(latest_budget_baseline_from_rollout_items(&rollout_items))
    }
    async fn save_history(&self, history: ConversationHistory) -> Result<()> {
        self.save_history(history).await
    }
    async fn persist_rollout_items(
        &self,
        conversation_id: &str,
        items: &[RolloutItem],
    ) -> Result<()> {
        self.persist_rollout_items(conversation_id, items).await
    }
    fn record_rollout_items(&self, conversation_id: &str, items: &[RolloutItem]) -> Result<()> {
        self.record_rollout_items(conversation_id, items)
    }
    async fn flush_rollout(&self) -> Result<()> {
        self.rollout_recorder.flush().await
    }
    fn should_persist_memory(&self, history: &ConversationHistory) -> bool {
        self.memory.should_persist(history)
    }
    fn persist_memory_from_history(&self, history: &ConversationHistory) {
        let _ = self.memory.persist_from_history(history);
    }

    async fn complete_model_request(
        &self,
        cancellation_token: &CancellationToken,
        request: crate::ModelRequest,
    ) -> Result<crate::ModelResponse> {
        complete_model_request(
            self.model.as_ref(),
            cancellation_token,
            request,
            TURN_INTERRUPTED_ERROR,
        )
        .await
    }

    async fn complete_model_request_streaming(
        &self,
        cancellation_token: &CancellationToken,
        request: crate::ModelRequest,
        observer: &mut dyn crate::ModelStreamObserver,
    ) -> Result<crate::ModelResponse> {
        complete_model_request_streaming(
            self.model.as_ref(),
            cancellation_token,
            request,
            observer,
            TURN_INTERRUPTED_ERROR,
        )
        .await
    }

    async fn run_tool_batch(
        &self,
        conversation_id: &str,
        turn_id: &str,
        permission_profile: &PermissionProfile,
        approval_policy: &ApprovalPolicy,
        cancellation_token: CancellationToken,
        tool_calls: Vec<ToolCall>,
        tool_specs: &[ToolSpec],
        discoverable_tools: &[ToolSpec],
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
        approval: &dyn ServerRequestHandler,
        denied_requests: &mut HashSet<String>,
    ) -> Result<crate::turn::ToolBatchOutcome> {
        crate::tool::run_host_tool_batch(
            self,
            conversation_id,
            turn_id,
            permission_profile,
            approval_policy,
            cancellation_token,
            tool_calls,
            tool_specs,
            discoverable_tools,
            context_manager,
            events,
            on_event,
            approval,
            denied_requests,
        )
        .await
    }

    fn audit_turn_started(&self, conversation_id: &str, user_input: &[crate::InputItem]) {
        self.append_audit(
            conversation_id,
            None,
            "turn.started",
            "info",
            json!({
                "input_preview": input_items_preview_text(user_input, 300),
                "input_items_count": user_input.len(),
                "attachment_count": input_items_attachment_count(user_input),
            }),
        );
    }
    fn audit_turn_completed(
        &self,
        conversation_id: &str,
        turn_id: &str,
        state: &str,
        events_count: usize,
        model_name: Option<&str>,
    ) {
        self.append_audit(
            conversation_id,
            Some(turn_id),
            "turn.completed",
            "info",
            json!({ "state": state, "events_count": events_count, "model": model_name }),
        );
    }
    fn audit_turn_cancelled(&self, conversation_id: &str, turn_id: &str, reason: &str) {
        self.append_audit(
            conversation_id,
            Some(turn_id),
            "turn.cancelled",
            "warn",
            json!({ "reason": reason }),
        );
    }
    fn audit_turn_failed(&self, conversation_id: &str, turn_id: &str, error: &str) {
        self.append_audit(
            conversation_id,
            Some(turn_id),
            "turn.failed",
            "error",
            json!({ "error": error.chars().take(1200).collect::<String>() }),
        );
    }
    fn audit_model_request_started(
        &self,
        conversation_id: &str,
        turn_id: &str,
        message_count: usize,
        tool_count: usize,
    ) {
        self.append_audit(
            conversation_id,
            Some(turn_id),
            "model.requested",
            "info",
            json!({ "message_count": message_count, "tool_count": tool_count }),
        );
    }
    fn audit_model_response_received(
        &self,
        conversation_id: &str,
        turn_id: &str,
        model_name: Option<&str>,
        has_content: bool,
        tool_call_count: usize,
    ) {
        self.append_audit(conversation_id, Some(turn_id), "model.responded", "info", json!({ "model_name": model_name, "has_content": has_content, "tool_call_count": tool_call_count }));
    }
}

fn is_placeholder_history(history: &ConversationHistory) -> bool {
    history.turn_count == 0 && matches!(history.messages.as_slice(), [ResponseItem::System { .. }])
}

fn latest_budget_baseline_from_rollout_items(
    rollout_items: &[RolloutItem],
) -> Option<RestoredBudgetBaseline> {
    rollout_items.iter().rev().find_map(|item| match item {
        RolloutItem::EventMsg {
            event:
                EventMsg::TokenUsageUpdated {
                    last_usage,
                    request_estimated_tokens,
                    ..
                },
        } => Some(RestoredBudgetBaseline {
            sdk_total_tokens: last_usage.total_tokens as usize,
            request_estimated_tokens: *request_estimated_tokens as usize,
        }),
        _ => None,
    })
}
