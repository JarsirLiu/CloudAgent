use crate::conversation::{ResponseItem, input_items_to_plain_text};

use super::CompactionContinuation;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ModelRequestShapeAudit {
    pub message_count: usize,
    pub tool_count: usize,
    pub compaction_phase: Option<&'static str>,
    pub message_roles: Vec<&'static str>,
    pub summary_index: Option<usize>,
    pub raw_tool_messages_before_summary: usize,
    pub raw_assistant_messages_before_summary: usize,
    pub latest_real_user_index: Option<usize>,
}

pub fn build_model_request_shape_audit(
    messages: &[ResponseItem],
    tool_count: usize,
    compaction_continuation: Option<CompactionContinuation>,
) -> ModelRequestShapeAudit {
    let summary_index = messages.iter().position(response_item_is_context_summary);
    let before_summary = summary_index.unwrap_or(messages.len());

    ModelRequestShapeAudit {
        message_count: messages.len(),
        tool_count,
        compaction_phase: compaction_continuation.map(compaction_phase),
        message_roles: messages.iter().map(response_item_role).collect(),
        summary_index,
        raw_tool_messages_before_summary: messages[..before_summary]
            .iter()
            .filter(|item| matches!(item, ResponseItem::Tool { .. }))
            .count(),
        raw_assistant_messages_before_summary: messages[..before_summary]
            .iter()
            .filter(|item| matches!(item, ResponseItem::Assistant { .. }))
            .count(),
        latest_real_user_index: messages
            .iter()
            .rposition(response_item_is_real_user_message),
    }
}

fn compaction_phase(continuation: CompactionContinuation) -> &'static str {
    match continuation {
        CompactionContinuation::PreTurn => "pre_turn",
        CompactionContinuation::MidTurn => "mid_turn",
    }
}

fn response_item_role(item: &ResponseItem) -> &'static str {
    match item {
        ResponseItem::System { .. } => "system",
        ResponseItem::User { .. } => "user",
        ResponseItem::Assistant { .. } => "assistant",
        ResponseItem::Tool { .. } => "tool",
    }
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

fn response_item_is_real_user_message(item: &ResponseItem) -> bool {
    let ResponseItem::User { content } = item else {
        return false;
    };
    let text = input_items_to_plain_text(content);
    let trimmed = text.trim_start();
    !trimmed.starts_with("[Context Summary]") && !trimmed.starts_with("<turn_aborted>")
}
