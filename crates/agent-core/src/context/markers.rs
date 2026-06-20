use crate::conversation::{ConversationHistory, ResponseItem, input_items_to_plain_text};
use crate::text_input_items;

pub fn context_summary_prefix() -> &'static str {
    "[Context Summary]"
}

pub fn turn_aborted_marker_text() -> &'static str {
    concat!(
        "<turn_aborted>\n",
        "The user interrupted the previous turn on purpose. Any running commands or tools may ",
        "have partially executed. Continue from the latest user request without assuming the ",
        "interrupted turn completed.\n",
        "</turn_aborted>"
    )
}

pub fn turn_aborted_marker_item() -> ResponseItem {
    ResponseItem::User {
        content: text_input_items(turn_aborted_marker_text()),
    }
}

pub fn append_turn_aborted_marker_if_needed(history: &mut ConversationHistory) {
    let already_marked = history.messages.last().is_some_and(is_turn_aborted_marker);
    if !already_marked {
        history.messages.push(turn_aborted_marker_item());
    }
}

pub fn is_context_summary_item(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::System { content } => {
            content.trim_start().starts_with(context_summary_prefix())
        }
        ResponseItem::User { content } => input_items_to_plain_text(content)
            .trim_start()
            .starts_with(context_summary_prefix()),
        ResponseItem::Assistant { .. } | ResponseItem::Tool { .. } => false,
    }
}

pub fn is_turn_aborted_marker(item: &ResponseItem) -> bool {
    let ResponseItem::User { content } = item else {
        return false;
    };
    input_items_to_plain_text(content)
        .trim_start()
        .starts_with("<turn_aborted>")
}

pub fn counts_as_real_user_turn(item: &ResponseItem) -> bool {
    matches!(item, ResponseItem::User { .. })
        && !is_context_summary_item(item)
        && !is_turn_aborted_marker(item)
}
