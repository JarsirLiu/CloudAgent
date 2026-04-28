use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type TurnId = String;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FrontendMode {
    Idle,
    Running,
    WaitingForApproval,
}

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
    pub turn_state: Option<TurnState>,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub mutating: bool,
    pub requires_approval: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
    pub summary: String,
    pub is_error: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistoryEntry {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
        has_tool_calls: bool,
    },
    Tool {
        tool_call_id: String,
        name: String,
        content: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnResultEnvelope {
    pub final_response: String,
    pub state: TurnState,
    pub error: Option<String>,
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
    TurnCancelled {
        turn_id: TurnId,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppClientCommand {
    SubmitTurn(UserTurnInput),
    ApprovalResponse {
        session_id: String,
        approved: bool,
        reason: Option<String>,
    },
    InterruptTurn {
        session_id: String,
    },
    ResetSession {
        session_id: String,
    },
    RequestStatus {
        session_id: String,
    },
    RequestHistory {
        session_id: String,
    },
    Exit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerEvent {
    FrontendStateChanged {
        session_id: String,
        mode: FrontendMode,
    },
    TurnEvent {
        session_id: String,
        event: TurnEvent,
    },
    ApprovalPrompt {
        session_id: String,
        request: ApprovalRequest,
    },
    SessionStatus {
        session_id: String,
        snapshot: SessionSnapshot,
    },
    SessionHistory {
        session_id: String,
        messages: Vec<HistoryEntry>,
    },
    TurnFinished {
        session_id: String,
        result: TurnResultEnvelope,
    },
    Info {
        session_id: String,
        message: String,
    },
    Error {
        session_id: String,
        message: String,
    },
}
