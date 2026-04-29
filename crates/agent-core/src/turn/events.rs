use crate::conversation::TranscriptItem;
use crate::protocol::RequestId;
use serde::{Deserialize, Serialize};

pub type TurnId = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolApprovalRequest {
    pub turn_id: TurnId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub arguments_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerRequestDecision {
    pub approved: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "request_type", rename_all = "snake_case")]
pub enum ServerRequest {
    ToolApproval { request: ToolApprovalRequest },
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Idle,
    Running,
    WaitingForServerRequest,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemKind {
    UserMessage,
    AssistantMessage,
    CommandExecution,
    FileChange,
    ToolCall,
    ToolResult,
    Reasoning,
    SystemNote,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemDeltaKind {
    Text,
    CommandExecutionOutput,
    ToolOutput,
    FileChangeOutput,
    ReasoningText,
    ReasoningSummary,
    JsonPatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMsg {
    TurnStarted {
        turn_id: TurnId,
        conversation_id: String,
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
    ItemStarted {
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
    },
    ItemDelta {
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemDeltaKind,
        delta: String,
    },
    ItemCompleted {
        turn_id: TurnId,
        item_id: String,
        item: TranscriptItem,
    },
    ServerRequestRequested {
        turn_id: TurnId,
        request: ServerRequest,
    },
    ServerRequestResolved {
        turn_id: TurnId,
        request: ServerRequest,
        decision: ServerRequestDecision,
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

#[derive(Clone, Debug)]
pub struct PendingTurnRequest {
    pub request_id: RequestId,
    pub request: ServerRequest,
}
