use crate::{ConversationHistory, ResponseItem, input_items_are_blank};

pub fn visible_message_count(history: &ConversationHistory) -> usize {
    history
        .messages
        .iter()
        .filter(|message| match message {
            ResponseItem::User { content } => !input_items_are_blank(content),
            ResponseItem::Assistant { content, .. } => content
                .as_deref()
                .is_some_and(|content| !content.trim().is_empty()),
            ResponseItem::System { .. } | ResponseItem::Tool { .. } => false,
        })
        .count()
}
