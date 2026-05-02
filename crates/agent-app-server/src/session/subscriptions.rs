use std::collections::HashSet;

#[derive(Debug)]
pub(crate) struct ConversationSubscriptions {
    subscribed_conversations: HashSet<String>,
}

impl ConversationSubscriptions {
    pub(crate) fn new(default_conversation_id: String) -> Self {
        let mut subscribed_conversations = HashSet::new();
        subscribed_conversations.insert(default_conversation_id);
        Self {
            subscribed_conversations,
        }
    }

    pub(crate) fn is_subscribed(&self, conversation_id: &str) -> bool {
        self.subscribed_conversations.contains(conversation_id)
    }

    pub(crate) fn subscribe(&mut self, conversation_id: String) {
        self.subscribed_conversations.insert(conversation_id);
    }

    pub(crate) fn unsubscribe(&mut self, conversation_id: &str) {
        self.subscribed_conversations.remove(conversation_id);
    }
}
