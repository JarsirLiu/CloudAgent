use crate::session::AgentSession;
use crate::tool::{ToolCall, ToolResult};
use serde::{Deserialize, Serialize};

pub type TurnId = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserTurnInput {
    pub session_id: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub turn_id: TurnId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub arguments_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub approved: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TurnState {
    Idle,
    Running,
    WaitingForApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionState {
    Idle,
    Busy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub session_state: SessionState,
    pub active_turn: Option<TurnId>,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnEvent {
    TurnStarted {
        turn_id: TurnId,
        session_id: String,
        user_input: String,
    },
    ModelRequestStarted {
        turn_id: TurnId,
        message_count: usize,
        tool_count: usize,
    },
    ModelResponseReceived {
        turn_id: TurnId,
        model_name: Option<String>,
        has_content: bool,
        tool_call_count: usize,
    },
    AssistantMessage {
        turn_id: TurnId,
        content: String,
    },
    ToolCallRequested {
        turn_id: TurnId,
        call: ToolCall,
    },
    ApprovalRequested {
        turn_id: TurnId,
        request: ApprovalRequest,
    },
    ApprovalResolved {
        turn_id: TurnId,
        tool_call_id: String,
        approved: bool,
        reason: Option<String>,
    },
    ToolCallCompleted {
        turn_id: TurnId,
        result: ToolResult,
    },
    ToolCallFailed {
        turn_id: TurnId,
        tool_call_id: String,
        tool_name: String,
        error: String,
    },
    TurnCompleted {
        turn_id: TurnId,
        final_response: String,
    },
    TurnFailed {
        turn_id: TurnId,
        error: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnOutcome {
    pub turn_id: TurnId,
    pub final_response: String,
    pub events: Vec<TurnEvent>,
    pub session: AgentSession,
    pub model_name: Option<String>,
    pub state: TurnState,
}
