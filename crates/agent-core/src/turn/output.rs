use crate::tool::ToolEvent;
use agent_protocol::{TurnEvent, TurnId, TurnState};

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
