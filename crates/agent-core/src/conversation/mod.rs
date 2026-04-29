mod history;
mod items;
mod state;

pub use history::{ConversationHistory, ResponseItem};
pub use items::{ConversationTurn, TranscriptItem};
pub use state::{
    ActiveConversationTurn, ConversationState, PendingConversationRequest, PersistedConversation,
};

pub fn module_name() -> &'static str {
    "agent-core::conversation"
}
