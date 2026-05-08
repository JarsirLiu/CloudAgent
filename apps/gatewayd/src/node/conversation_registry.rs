use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct ConversationRegistry {
    active_conversation_id: String,
    known_conversations: HashMap<String, WorkerConversationHandle>,
}

impl ConversationRegistry {
    pub(crate) fn new(active_conversation_id: String) -> Self {
        let mut registry = Self {
            active_conversation_id: active_conversation_id.clone(),
            known_conversations: HashMap::new(),
        };
        registry.touch(&active_conversation_id);
        registry
    }

    pub(crate) fn active_conversation_id(&self) -> &str {
        &self.active_conversation_id
    }

    pub(crate) fn set_active_conversation(&mut self, conversation_id: impl Into<String>) {
        let conversation_id = conversation_id.into();
        self.touch(&conversation_id);
        self.active_conversation_id = conversation_id;
    }

    pub(crate) fn touch(&mut self, conversation_id: &str) {
        self.known_conversations
            .entry(conversation_id.to_string())
            .or_insert_with(|| WorkerConversationHandle {
                conversation_id: conversation_id.to_string(),
            });
    }

    pub(crate) fn known_conversation_ids(&self) -> Vec<String> {
        self.known_conversations
            .values()
            .map(|handle| handle.conversation_id.clone())
            .collect()
    }
}

pub(crate) struct WorkerConversationHandle {
    pub(crate) conversation_id: String,
}

#[cfg(test)]
mod tests {
    use super::ConversationRegistry;

    #[test]
    fn tracks_active_conversation_and_known_ids() {
        let mut registry = ConversationRegistry::new("conversation-a".to_string());
        registry.touch("conversation-b");
        registry.set_active_conversation("conversation-b");

        assert_eq!(registry.active_conversation_id(), "conversation-b");
        let mut ids = registry.known_conversation_ids();
        ids.sort();
        assert_eq!(
            ids,
            vec!["conversation-a".to_string(), "conversation-b".to_string()]
        );
    }
}
