use crate::conversation::ResponseItem;
use crate::conversation::input_items_to_plain_text;

pub trait ContextFragment {
    fn render(&self) -> ResponseItem;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextInjectionStrategy {
    Standard,
    MidTurnCompactionContinuation,
}

pub(crate) fn insert_context_fragments_before_latest_user(
    mut messages: Vec<ResponseItem>,
    fragments: &[ResponseItem],
) -> Vec<ResponseItem> {
    if fragments.is_empty() {
        return messages;
    }

    let insert_at = messages
        .iter()
        .rposition(response_item_is_real_user_message)
        .or_else(|| messages.iter().rposition(response_item_is_context_summary))
        .unwrap_or(messages.len());
    messages.splice(insert_at..insert_at, fragments.iter().cloned());
    messages
}

pub(crate) fn insert_context_fragments(
    messages: Vec<ResponseItem>,
    fragments: &[ResponseItem],
    strategy: ContextInjectionStrategy,
) -> Vec<ResponseItem> {
    match strategy {
        ContextInjectionStrategy::Standard => {
            insert_context_fragments_before_latest_user(messages, fragments)
        }
        ContextInjectionStrategy::MidTurnCompactionContinuation => {
            insert_context_fragments_for_mid_turn_compaction(messages, fragments)
        }
    }
}

fn insert_context_fragments_for_mid_turn_compaction(
    mut messages: Vec<ResponseItem>,
    fragments: &[ResponseItem],
) -> Vec<ResponseItem> {
    if fragments.is_empty() {
        return messages;
    }

    let insert_at = messages
        .iter()
        .rposition(response_item_is_real_user_message)
        .or_else(|| messages.iter().rposition(response_item_is_context_summary))
        .unwrap_or(messages.len());
    messages.splice(insert_at..insert_at, fragments.iter().cloned());
    messages
}

fn response_item_is_real_user_message(item: &ResponseItem) -> bool {
    let ResponseItem::User { content } = item else {
        return false;
    };
    let text = input_items_to_plain_text(content);
    let trimmed = text.trim_start();
    !trimmed.starts_with("[Context Summary]") && !trimmed.starts_with("<turn_aborted>")
}

fn response_item_is_context_summary(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::System { content } => content.trim_start().starts_with("[Context Summary]"),
        ResponseItem::User { content } => input_items_to_plain_text(content)
            .trim_start()
            .starts_with("[Context Summary]"),
        ResponseItem::Assistant { .. } | ResponseItem::Tool { .. } => false,
    }
}
