use agent_core::conversation::ConversationSummary;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub(crate) struct ConversationRegistry {
    known_conversations: HashMap<String, ConversationMeta>,
}

impl ConversationRegistry {
    pub(crate) fn touch(&mut self, conversation_id: &str) {
        let now = now_ms();
        self.known_conversations
            .entry(conversation_id.to_string())
            .and_modify(|meta| meta.updated_at_ms = now)
            .or_insert_with(|| ConversationMeta {
                conversation_id: conversation_id.to_string(),
                title: None,
                message_count: 0,
                updated_at_ms: now,
            });
    }

    pub(crate) fn set_title(&mut self, conversation_id: &str, title: String) {
        self.touch(conversation_id);
        if let Some(meta) = self.known_conversations.get_mut(conversation_id) {
            meta.title = Some(title);
        }
    }

    pub(crate) fn summaries(&self) -> Vec<ConversationSummary> {
        let mut summaries: Vec<_> = self
            .known_conversations
            .values()
            .map(|meta| ConversationSummary {
                conversation_id: meta.conversation_id.clone(),
                title: meta.title.clone(),
                message_count: meta.message_count,
                updated_at_ms: meta.updated_at_ms,
            })
            .collect();
        summaries.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| left.conversation_id.cmp(&right.conversation_id))
        });
        summaries
    }
}

struct ConversationMeta {
    conversation_id: String,
    title: Option<String>,
    message_count: usize,
    updated_at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::ConversationRegistry;

    #[test]
    fn summaries_include_touched_conversations_in_recent_order() {
        let mut registry = ConversationRegistry::default();
        registry.touch("conversation-a");
        registry.touch("conversation-b");
        registry.set_title("conversation-a", "Alpha".to_string());

        let summaries = registry.summaries();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].conversation_id, "conversation-a");
        assert_eq!(summaries[0].title.as_deref(), Some("Alpha"));
        assert_eq!(summaries[1].conversation_id, "conversation-b");
    }
}
