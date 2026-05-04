mod history;
mod items;
mod metrics;
mod state;

pub use history::{ConversationHistory, ResponseItem};
pub use items::{ConversationTurn, TranscriptItem};
pub use metrics::visible_message_count;
use serde::{Deserialize, Serialize};
pub use state::{
    ActiveConversationTurn, ConversationState, PendingConversationRequest,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConversationStatus {
    Idle,
    Busy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationSnapshot {
    pub conversation_id: String,
    pub conversation_status: ConversationStatus,
    pub active_turn: Option<String>,
    pub turn_state: Option<crate::turn::TurnState>,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub title: Option<String>,
    pub message_count: usize,
    pub updated_at_ms: u64,
}
