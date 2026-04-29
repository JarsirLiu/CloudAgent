mod events;
mod lifecycle;
mod output;

pub use events::{
    PendingTurnRequest, ServerRequest, ServerRequestDecision, ToolApprovalRequest, TurnEvent,
    TurnId, TurnItemDeltaKind, TurnItemKind, TurnState,
};
pub use lifecycle::{TurnLifecycleClass, TurnLifecyclePhase};
pub use output::AgentTurnOutput;

pub fn module_name() -> &'static str {
    "agent-core::turn"
}
