use crate::conversation::ResponseItem;

pub trait ContextFragment {
    fn render(&self) -> ResponseItem;
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
        .rposition(|item| matches!(item, ResponseItem::User { .. }))
        .unwrap_or(messages.len());
    messages.splice(insert_at..insert_at, fragments.iter().cloned());
    messages
}
