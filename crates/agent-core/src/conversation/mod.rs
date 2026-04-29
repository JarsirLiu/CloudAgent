mod history;
mod state;

pub use history::{ConversationHistory, ConversationMessage};
pub use state::{
    ActiveConversationTurn, ConversationState, PendingConversationRequest, PersistedConversation,
};

pub fn module_name() -> &'static str {
    "agent-core::conversation"
}
