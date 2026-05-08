use crate::conversation::ResponseItem;

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
        .rposition(|item| matches!(item, ResponseItem::User { .. }))
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
        .rposition(|item| matches!(item, ResponseItem::User { .. }))
        .or_else(|| {
            messages.iter().rposition(|item| {
                matches!(
                    item,
                    ResponseItem::System { content }
                        if content.trim_start().starts_with("[Context Summary]")
                )
            })
        })
        .unwrap_or(messages.len());
    messages.splice(insert_at..insert_at, fragments.iter().cloned());
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{input_items_to_plain_text, text_input_items};

    #[test]
    fn mid_turn_compaction_inserts_before_summary_when_tail_has_no_real_user() {
        let messages = vec![
            ResponseItem::System {
                content: "system".to_string(),
            },
            ResponseItem::System {
                content: "[Context Summary]\nsummary".to_string(),
            },
            ResponseItem::Assistant {
                content: Some("assistant tail".to_string()),
                tool_calls: Vec::new(),
            },
        ];
        let fragments = vec![ResponseItem::User {
            content: text_input_items("<environment_context>\nctx"),
        }];

        let injected = insert_context_fragments(
            messages,
            &fragments,
            ContextInjectionStrategy::MidTurnCompactionContinuation,
        );

        assert!(matches!(
            &injected[..],
            [
                ResponseItem::System { content: system },
                ResponseItem::User { content: env },
                ResponseItem::System { content: summary },
                ResponseItem::Assistant { content: Some(assistant_tail), .. },
            ] if system == "system"
                && input_items_to_plain_text(env).starts_with("<environment_context>")
                && summary == "[Context Summary]\nsummary"
                && assistant_tail == "assistant tail"
        ));
    }
}
