use agent_core::ConversationStatus;
use agent_protocol::FrontendMode;
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct ConversationExecutionRegistry {
    modes_by_conversation: HashMap<String, FrontendMode>,
}

impl ConversationExecutionRegistry {
    pub(crate) fn update_frontend_mode(&mut self, conversation_id: &str, mode: FrontendMode) {
        if matches!(mode, FrontendMode::Idle) {
            self.modes_by_conversation.remove(conversation_id);
        } else {
            self.modes_by_conversation
                .insert(conversation_id.to_string(), mode);
        }
    }

    pub(crate) fn update_conversation_status(
        &mut self,
        conversation_id: &str,
        status: &ConversationStatus,
    ) {
        match status {
            ConversationStatus::Idle => {
                self.modes_by_conversation.remove(conversation_id);
            }
            ConversationStatus::Busy => {
                self.modes_by_conversation
                    .entry(conversation_id.to_string())
                    .or_insert(FrontendMode::Running);
            }
        }
    }

    pub(crate) fn is_busy(&self, conversation_id: &str) -> bool {
        self.modes_by_conversation.contains_key(conversation_id)
    }
}

#[cfg(test)]
mod tests {
    use super::ConversationExecutionRegistry;
    use agent_core::ConversationStatus;
    use agent_protocol::FrontendMode;

    #[test]
    fn running_and_waiting_modes_are_tracked_as_busy() {
        let mut registry = ConversationExecutionRegistry::default();

        registry.update_frontend_mode("conversation-1", FrontendMode::Running);
        assert!(registry.is_busy("conversation-1"));

        registry.update_frontend_mode("conversation-1", FrontendMode::WaitingForServerRequest);
        assert!(registry.is_busy("conversation-1"));
    }

    #[test]
    fn idle_mode_clears_busy_state() {
        let mut registry = ConversationExecutionRegistry::default();
        registry.update_frontend_mode("conversation-1", FrontendMode::Running);

        registry.update_frontend_mode("conversation-1", FrontendMode::Idle);

        assert!(!registry.is_busy("conversation-1"));
    }

    #[test]
    fn conversation_status_updates_busy_state() {
        let mut registry = ConversationExecutionRegistry::default();

        registry.update_conversation_status("conversation-1", &ConversationStatus::Busy);
        assert!(registry.is_busy("conversation-1"));

        registry.update_conversation_status("conversation-1", &ConversationStatus::Idle);
        assert!(!registry.is_busy("conversation-1"));
    }
}
