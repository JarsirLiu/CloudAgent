use crate::{ConversationHistory, ResponseItem};

pub fn visible_message_count(history: &ConversationHistory) -> usize {
    history
        .messages
        .iter()
        .filter(|message| match message {
            ResponseItem::User { content } => !content.trim().is_empty(),
            ResponseItem::Assistant { content, .. } => content
                .as_deref()
                .is_some_and(|content| !content.trim().is_empty()),
            ResponseItem::System { .. } | ResponseItem::Tool { .. } => false,
        })
        .count()
}
