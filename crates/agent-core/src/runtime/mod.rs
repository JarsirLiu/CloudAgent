use crate::protocol::{TurnEvent, TurnId, TurnState};
use crate::tool::ToolEvent;

#[derive(Clone, Debug)]
pub struct AgentTurnOutput {
    pub turn_id: TurnId,
    pub final_response: String,
    pub tool_events: Vec<ToolEvent>,
    pub events: Vec<TurnEvent>,
    pub model_name: Option<String>,
    pub total_messages: usize,
    pub state: TurnState,
}

pub fn module_name() -> &'static str {
    "agent-core::runtime"
}
