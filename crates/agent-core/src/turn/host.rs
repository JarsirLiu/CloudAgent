use crate::InputItem;
use crate::context::{ContextManager, EnvironmentContext};
use crate::conversation::ConversationHistory;
use crate::model::{ModelRequest, ModelResponse, ModelStreamObserver};
use crate::rollout::RolloutItem;
use crate::skill::SkillRuntime;
use crate::state::ActiveTurnHandle;
use crate::tool::{RegularTurnToolExposure, ToolCall, ToolSpec};
use crate::turn::{EventMsg, ServerRequest, ServerRequestDecision};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RestoredBudgetBaseline {
    pub sdk_total_tokens: usize,
    pub request_estimated_tokens: usize,
}

#[derive(Clone, Debug)]
pub struct RegularTurnSettings {
    pub workspace_root: PathBuf,
    pub data_root_dir: PathBuf,
    pub llm_temperature: f32,
    pub pre_llm_filter_enabled: bool,
    pub max_tool_roundtrips: Option<usize>,
    pub model_context_window: u64,
    pub context_compaction_trigger_ratio: f32,
    pub context_compaction_request_overhead_tokens: usize,
    pub context_compaction_target_tokens: usize,
    pub context_compaction_preserved_user_turns: usize,
    pub context_compaction_preserved_tail_tokens: usize,
    pub context_compaction_summary_source_tokens: usize,
    pub post_compact_token_budget: usize,
    pub post_compact_memory_floor_tokens: usize,
    pub post_compact_skills_token_budget: usize,
    pub post_compact_mcp_token_budget: usize,
    pub post_compact_max_tokens_per_memory: usize,
    pub post_compact_max_tokens_per_skill: usize,
    pub post_compact_max_tokens_per_mcp: usize,
    pub context_budget_safety_buffer_tokens: usize,
    pub enable_skill_bucket: bool,
    pub enable_mcp_bucket: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolBatchOutcome {
    pub cancelled: bool,
    pub exposed_tools: Vec<String>,
}

#[async_trait]
pub trait ServerRequestHandler: Send + Sync {
    async fn decide(&self, request: ServerRequest) -> Result<ServerRequestDecision>;
}

#[async_trait]
impl<F, Fut> ServerRequestHandler for F
where
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: Future<Output = Result<ServerRequestDecision>> + Send,
{
    async fn decide(&self, request: ServerRequest) -> Result<ServerRequestDecision> {
        (self)(request).await
    }
}

#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait TurnHost: Send + Sync {
    type PermissionProfile: Send + Sync;
    type ApprovalPolicy: Send + Sync;

    fn turn_interrupted_error(&self) -> &'static str;
    fn regular_turn_settings(&self) -> RegularTurnSettings;
    fn environment_context(&self) -> EnvironmentContext;
    fn raw_memory_fragment(&self) -> Option<String>;
    fn skills(&self) -> SkillRuntime;
    fn resolve_regular_turn_tool_exposure(
        &self,
        permission_profile: &Self::PermissionProfile,
    ) -> RegularTurnToolExposure;

    async fn start_turn(
        &self,
        conversation_id: String,
        turn_id: String,
    ) -> Option<ActiveTurnHandle>;
    async fn finish_turn(&self, conversation_id: &str);
    async fn is_turn_cancelled(&self, conversation_id: &str) -> bool;
    fn append_conversation_event(&self, conversation_id: &str, event: EventMsg);

    async fn load_history(&self, conversation_id: &str) -> Result<ConversationHistory>;
    async fn history_from_rollout(&self, conversation_id: &str) -> Result<ConversationHistory>;
    async fn restore_budget_baseline(
        &self,
        conversation_id: &str,
    ) -> Result<Option<RestoredBudgetBaseline>>;
    async fn save_history(&self, history: ConversationHistory) -> Result<()>;
    async fn persist_rollout_items(
        &self,
        conversation_id: &str,
        items: &[RolloutItem],
    ) -> Result<()>;
    fn record_rollout_items(&self, conversation_id: &str, items: &[RolloutItem]) -> Result<()>;
    async fn flush_rollout(&self) -> Result<()>;

    fn should_persist_memory(&self, history: &ConversationHistory) -> bool;
    fn persist_memory_from_history(&self, history: &ConversationHistory);

    async fn complete_model_request(
        &self,
        cancellation_token: &CancellationToken,
        request: ModelRequest,
    ) -> Result<ModelResponse>;
    async fn complete_model_request_streaming(
        &self,
        cancellation_token: &CancellationToken,
        request: ModelRequest,
        observer: &mut dyn ModelStreamObserver,
    ) -> Result<ModelResponse>;

    async fn run_tool_batch(
        &self,
        conversation_id: &str,
        turn_id: &str,
        permission_profile: &Self::PermissionProfile,
        approval_policy: &Self::ApprovalPolicy,
        cancellation_token: CancellationToken,
        tool_calls: Vec<ToolCall>,
        tool_specs: &[ToolSpec],
        discoverable_tools: &[ToolSpec],
        context_manager: &mut ContextManager,
        events: &mut Vec<EventMsg>,
        on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
        approval: &dyn ServerRequestHandler,
        denied_requests: &mut HashSet<String>,
    ) -> Result<ToolBatchOutcome>;

    fn audit_turn_started(&self, conversation_id: &str, user_input: &[InputItem]);
    fn audit_turn_completed(
        &self,
        conversation_id: &str,
        turn_id: &str,
        state: &str,
        events_count: usize,
        model_name: Option<&str>,
    );
    fn audit_turn_cancelled(&self, conversation_id: &str, turn_id: &str, reason: &str);
    fn audit_turn_failed(&self, conversation_id: &str, turn_id: &str, error: &str);
    fn audit_model_request_started(
        &self,
        conversation_id: &str,
        turn_id: &str,
        message_count: usize,
        tool_count: usize,
    );
    fn audit_model_response_received(
        &self,
        conversation_id: &str,
        turn_id: &str,
        model_name: Option<&str>,
        has_content: bool,
        tool_call_count: usize,
    );
}
