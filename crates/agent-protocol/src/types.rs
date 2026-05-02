use crate::{TurnId, TurnState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FrontendMode {
    Idle,
    Running,
    WaitingForServerRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserTurnInput {
    pub session_id: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionStatus {
    Idle,
    Busy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub conversation_status: SessionStatus,
    pub active_turn: Option<TurnId>,
    pub turn_state: Option<TurnState>,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub message_count: usize,
    pub updated_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDelivery {
    Lossless,
    BestEffort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationStream {
    CoreTranscript,
    Control,
    Diagnostic,
}
