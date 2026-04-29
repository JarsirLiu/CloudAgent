mod events;
mod lifecycle;
mod output;

pub use events::{
    EventMsg, PendingTurnRequest, ServerRequest, ServerRequestDecision, ToolApprovalRequest,
    TurnId, TurnItemDeltaKind, TurnItemKind, TurnState,
};
pub use lifecycle::{TurnLifecycleClass, TurnLifecyclePhase};
pub use output::AgentTurnOutput;

pub fn module_name() -> &'static str {
    "agent-core::turn"
}
