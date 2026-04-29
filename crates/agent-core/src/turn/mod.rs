mod lifecycle;
mod output;

pub use lifecycle::{TurnLifecycleClass, TurnLifecyclePhase};
pub use output::AgentTurnOutput;

pub fn module_name() -> &'static str {
    "agent-core::turn"
}
