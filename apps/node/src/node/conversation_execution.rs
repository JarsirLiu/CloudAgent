use agent_protocol::ConversationViewStatus;
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct ConversationExecutionRegistry {
    active_conversations: HashMap<String, ()>,
}

impl ConversationExecutionRegistry {
    pub(crate) fn update_conversation_view(
        &mut self,
        conversation_id: &str,
        status: &ConversationViewStatus,
    ) {
        match status {
            ConversationViewStatus::Active { .. } => {
                self.active_conversations
                    .insert(conversation_id.to_string(), ());
            }
            ConversationViewStatus::NotLoaded
            | ConversationViewStatus::Idle
            | ConversationViewStatus::SystemError { .. } => {
                self.active_conversations.remove(conversation_id);
            }
        }
    }

    pub(crate) fn is_busy(&self, conversation_id: &str) -> bool {
        self.active_conversations.contains_key(conversation_id)
    }
}

#[cfg(test)]
#[path = "conversation_execution_tests.rs"]
mod conversation_execution_tests;
