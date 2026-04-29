mod history;
mod items;
mod state;

pub use history::{ConversationHistory, ConversationMessage};
pub use items::{HistoryEntry, ThreadItem};
pub use state::{
    ActiveConversationTurn, ConversationState, PendingConversationRequest, PersistedConversation,
};

pub fn module_name() -> &'static str {
    "agent-core::conversation"
}
