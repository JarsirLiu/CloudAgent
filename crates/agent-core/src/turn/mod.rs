mod api;
pub(crate) mod compaction;
mod concurrency;
mod events;
mod execution;
mod host;
mod lifecycle;
pub(crate) mod loop_guard;
mod model_request_audit;
mod orchestrator;
mod outcome;
mod output;
mod policy;
mod request_id;
pub(crate) mod token_usage;
mod utils;

#[cfg(test)]
mod model_request_audit_tests;

pub use api::{chat, chat_with_approval, chat_with_approval_and_events, compact_conversation};
pub use compaction::{
    AutoCompactPolicyInput, AutoCompactTokenLimitScope, AutoCompactTokenStatus,
    auto_compact_token_status,
};
pub use compaction::{
    AutoCompactWindow, AutoCompactWindowSnapshot, CompactionOutcome, CompactionPhase,
    CompactionReason, CompactionRequest, CompactionTrigger, InitialContextInjection,
    ManualCompactionOutcome, run_manual_compaction,
};
pub use concurrency::{
    CONVERSATION_BUSY_ERROR_CODE, CONVERSATION_BUSY_ERROR_MESSAGE, conversation_busy_error,
};
pub use events::{
    CommandApprovalRequest, EventMsg, FileChangeApprovalRequest, ModelRetryStage,
    PendingTurnRequest, ServerRequest, ServerRequestDecision, ServerRequestDecisionKind, TurnId,
    TurnItemDeltaKind, TurnItemKind, TurnState,
};
pub use execution::execute_chat_turn;
pub use host::{ChatTurnSettings, ServerRequestHandler, ToolBatchOutcome, TurnHost};
pub use lifecycle::{TurnLifecycleClass, TurnLifecyclePhase};
pub use model_request_audit::{ModelRequestShapeAudit, build_model_request_shape_audit};
pub use orchestrator::run_turn_with_approval;
pub use outcome::{TurnOutcome, emit_assistant_message_item};
pub use output::AgentTurnOutput;
pub use policy::ExecutionPolicy;
pub use policy::{ApprovalPolicy, PermissionProfile, TurnPolicy, UserTurnInput};
pub use request_id::RequestId;
pub use token_usage::{
    RequestTokenBaseline, RestoredTurnTokenState, TokenUsageState, apply_signed_token_delta,
    latest_turn_token_state_from_rollout_items,
};
pub use utils::{emit_event, next_turn_id, paginate_turns};
