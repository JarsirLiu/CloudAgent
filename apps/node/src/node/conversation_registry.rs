use agent_core::conversation::{
    ConversationSummary, ConversationTurn, TranscriptItem, input_items_are_blank,
};
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

    pub(crate) fn replace_from_summaries(&mut self, summaries: &[ConversationSummary]) {
        self.known_conversations = summaries
            .iter()
            .map(|summary| {
                (
                    summary.conversation_id.clone(),
                    ConversationMeta {
                        conversation_id: summary.conversation_id.clone(),
                        title: summary.title.clone(),
                        message_count: summary.message_count,
                        updated_at_ms: summary.updated_at_ms,
                    },
                )
            })
            .collect();
    }

    pub(crate) fn update_from_history(
        &mut self,
        conversation_id: &str,
        turns: &[ConversationTurn],
    ) {
        let message_count = turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .filter(|item| is_visible_message(item))
            .count();

        let now = now_ms();
        self.known_conversations
            .entry(conversation_id.to_string())
            .and_modify(|meta| {
                meta.message_count = message_count;
                meta.updated_at_ms = now;
            })
            .or_insert_with(|| ConversationMeta {
                conversation_id: conversation_id.to_string(),
                title: None,
                message_count,
                updated_at_ms: now,
            });
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

fn is_visible_message(item: &TranscriptItem) -> bool {
    match item {
        TranscriptItem::UserMessage { content, .. } => !input_items_are_blank(content),
        TranscriptItem::AgentMessage { text, .. } => !text.trim().is_empty(),
        TranscriptItem::SystemMessage { .. }
        | TranscriptItem::CommandExecution { .. }
        | TranscriptItem::FileChange { .. }
        | TranscriptItem::ToolResult { .. }
        | TranscriptItem::Reasoning { .. } => false,
    }
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
    use agent_core::conversation::{
        ConversationSummary, ConversationTurn, InputItem, TranscriptItem,
    };

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

    #[test]
    fn replace_from_summaries_overwrites_stale_registry_entries() {
        let mut registry = ConversationRegistry::default();
        registry.touch("stale");
        registry.replace_from_summaries(&[ConversationSummary {
            conversation_id: "fresh".to_string(),
            title: Some("Fresh".to_string()),
            message_count: 3,
            updated_at_ms: 42,
        }]);

        let summaries = registry.summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].conversation_id, "fresh");
        assert_eq!(summaries[0].title.as_deref(), Some("Fresh"));
        assert_eq!(summaries[0].message_count, 3);
        assert_eq!(summaries[0].updated_at_ms, 42);
    }

    #[test]
    fn update_from_history_tracks_visible_message_count() {
        let mut registry = ConversationRegistry::default();
        registry.update_from_history(
            "conversation-a",
            &[ConversationTurn {
                id: "turn-1".to_string(),
                state: agent_core::TurnState::Completed,
                items: vec![
                    TranscriptItem::UserMessage {
                        id: "user-1".to_string(),
                        content: vec![InputItem::Text {
                            text: "hello".to_string(),
                        }],
                    },
                    TranscriptItem::Reasoning {
                        id: "reasoning-1".to_string(),
                        title: "Reasoning".to_string(),
                        text: "thinking".to_string(),
                    },
                    TranscriptItem::AgentMessage {
                        id: "assistant-1".to_string(),
                        text: "world".to_string(),
                    },
                ],
                rollout_start_index: 0,
                rollout_end_index: 0,
            }],
        );

        let summaries = registry.summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].message_count, 2);
    }
}
