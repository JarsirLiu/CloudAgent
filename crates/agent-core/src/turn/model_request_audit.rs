use crate::context::{counts_as_real_user_turn, is_context_summary_item};
use crate::conversation::ResponseItem;

use super::CompactionPhase;

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
    compaction_phase: Option<CompactionPhase>,
) -> ModelRequestShapeAudit {
    let summary_index = messages.iter().position(is_context_summary_item);
    let before_summary = summary_index.unwrap_or(messages.len());

    ModelRequestShapeAudit {
        message_count: messages.len(),
        tool_count,
        compaction_phase: compaction_phase.map(compaction_phase_name),
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
        latest_real_user_index: messages.iter().rposition(counts_as_real_user_turn),
    }
}

fn compaction_phase_name(phase: CompactionPhase) -> &'static str {
    match phase {
        CompactionPhase::StandaloneTurn => "standalone_turn",
        CompactionPhase::PreTurn => "pre_turn",
        CompactionPhase::MidTurn => "mid_turn",
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
