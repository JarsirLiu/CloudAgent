use crate::context::{counts_as_real_user_turn, is_context_summary_item};
use crate::conversation::ResponseItem;

pub trait ContextFragment {
    fn render(&self) -> ResponseItem;
}

pub(crate) fn insert_context_fragments(
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
    counts_as_real_user_turn(item)
}

fn response_item_is_context_summary(item: &ResponseItem) -> bool {
    is_context_summary_item(item)
}
