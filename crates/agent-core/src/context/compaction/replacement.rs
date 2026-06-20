use crate::context::counts_as_real_user_turn;
use crate::conversation::{ResponseItem, text_input_items};

use super::{CompactionSummary, ContextCompactionPlan};

#[derive(Clone, Debug)]
pub struct ContextCompactionResult {
    pub summary: CompactionSummary,
    pub replacement_history: Vec<ResponseItem>,
}

#[derive(Clone, Debug)]
pub struct CompactedReplacementHistory {
    pub messages: Vec<ResponseItem>,
    pub preserved_user_count: usize,
}

pub fn build_compacted_replacement_history(
    source_messages: &[ResponseItem],
    plan: &ContextCompactionPlan,
    summary: &CompactionSummary,
) -> CompactedReplacementHistory {
    let system_prompt = source_messages
        .first()
        .cloned()
        .unwrap_or_else(|| ResponseItem::System {
            content: String::new(),
        });

    let mut messages = vec![system_prompt];
    messages.extend(recent_real_user_messages(&plan.preserved_tail));
    let preserved_user_count = messages.len().saturating_sub(1);
    messages.push(ResponseItem::User {
        content: text_input_items(summary.rendered()),
    });

    CompactedReplacementHistory {
        messages,
        preserved_user_count,
    }
}

pub fn apply_history_compaction(
    messages: &mut Vec<ResponseItem>,
    plan: &ContextCompactionPlan,
    summary: CompactionSummary,
) -> ContextCompactionResult {
    let replacement = build_compacted_replacement_history(messages, plan, &summary).messages;

    *messages = replacement.clone();

    ContextCompactionResult {
        summary,
        replacement_history: replacement,
    }
}

fn recent_real_user_messages(messages: &[ResponseItem]) -> Vec<ResponseItem> {
    messages
        .iter()
        .filter_map(|item| match item {
            ResponseItem::User { content } if counts_as_real_user_turn(item) => {
                Some(ResponseItem::User {
                    content: content.clone(),
                })
            }
            _ => None,
        })
        .collect()
}
