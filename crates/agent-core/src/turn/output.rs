use crate::tool::ToolEvent;
use crate::turn::{EventMsg, TurnId, TurnState};

#[derive(Clone, Debug)]
pub struct AgentTurnOutput {
    pub turn_id: TurnId,
    pub tool_events: Vec<ToolEvent>,
    pub events: Vec<EventMsg>,
    pub model_name: Option<String>,
    pub total_messages: usize,
    pub state: TurnState,
}
