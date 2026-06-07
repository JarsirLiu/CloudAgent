use crate::RequestId;
use agent_core::conversation::ConversationTurn;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ConversationViewStatus {
    NotLoaded,
    Idle,
    Active {
        active_turn_id: Option<String>,
        flags: Vec<ConversationActiveFlag>,
    },
    SystemError {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConversationActiveFlag {
    RunningTurn,
    WaitingOnApproval,
    WaitingOnUserInput,
    InterruptRequested,
    CompactingContext,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnViewStatus {
    InProgress,
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ServerRequestViewKind {
    CommandApproval,
    FileChangeApproval,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingServerRequestView {
    pub request_id: RequestId,
    pub conversation_id: String,
    pub turn_id: String,
    pub kind: ServerRequestViewKind,
    pub tool_name: String,
    pub reason: String,
    pub preview: String,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationViewSnapshot {
    pub conversation_id: String,
    pub status: ConversationViewStatus,
    pub active_turn: Option<ConversationTurn>,
    pub pending_requests: Vec<PendingServerRequestView>,
    pub message_count: usize,
    pub updated_at_ms: u64,
}
