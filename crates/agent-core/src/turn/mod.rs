mod api;
mod auto_compact_policy;
mod auto_compact_window;
mod compaction;
mod concurrency;
mod events;
mod host;
mod lifecycle;
mod loop_guard;
mod model_request_audit;
mod orchestrator;
mod outcome;
mod output;
mod policy;
mod regular;
mod request_id;
mod token_usage;
mod utils;

#[cfg(test)]
mod auto_compact_policy_tests;
#[cfg(test)]
mod auto_compact_window_tests;
#[cfg(test)]
mod model_request_audit_tests;

pub use api::{chat, chat_with_approval, chat_with_approval_and_events, compact_conversation};
pub use auto_compact_policy::{
    AutoCompactPolicyInput, AutoCompactTokenLimitScope, AutoCompactTokenStatus,
    auto_compact_token_status,
};
pub use auto_compact_window::{AutoCompactWindow, AutoCompactWindowSnapshot};
pub use compaction::{CompactionContinuation, ManualCompactionOutcome, run_manual_compaction};
pub use concurrency::{
    CONVERSATION_BUSY_ERROR_CODE, CONVERSATION_BUSY_ERROR_MESSAGE, conversation_busy_error,
};
pub use events::{
    CommandApprovalRequest, EventMsg, FileChangeApprovalRequest, ModelRetryStage,
    PendingTurnRequest, ServerRequest, ServerRequestDecision, ServerRequestDecisionKind, TurnId,
    TurnItemDeltaKind, TurnItemKind, TurnState,
};
pub use host::{RegularTurnSettings, ServerRequestHandler, ToolBatchOutcome, TurnHost};
pub use lifecycle::{TurnLifecycleClass, TurnLifecyclePhase};
pub use model_request_audit::{ModelRequestShapeAudit, build_model_request_shape_audit};
pub use orchestrator::run_turn_with_approval;
pub use outcome::{TurnOutcome, emit_assistant_message_item};
pub use output::AgentTurnOutput;
pub use policy::ExecutionPolicy;
pub use policy::{ApprovalPolicy, PermissionProfile, TurnPolicy, UserTurnInput};
pub use regular::execute_regular_turn;
pub use request_id::RequestId;
pub use token_usage::{
    RequestTokenBaseline, RestoredTurnTokenState, TokenUsageState, apply_signed_token_delta,
    latest_turn_token_state_from_rollout_items,
};
pub use utils::{emit_event, next_turn_id, paginate_turns};
