use crate::conversation::{ResponseItem, input_items_to_plain_text, text_input_items};

use super::{CompactionSummary, ContextCompactionPlan};

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

fn recent_real_user_messages(messages: &[ResponseItem]) -> Vec<ResponseItem> {
    messages
        .iter()
        .filter_map(|item| match item {
            ResponseItem::User { content } if user_content_is_real(content) => {
                Some(ResponseItem::User {
                    content: content.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

fn user_content_is_real(content: &[crate::InputItem]) -> bool {
    let text = input_items_to_plain_text(content);
    let trimmed = text.trim_start();
    !trimmed.starts_with("[Context Summary]") && !trimmed.starts_with("<turn_aborted>")
}
