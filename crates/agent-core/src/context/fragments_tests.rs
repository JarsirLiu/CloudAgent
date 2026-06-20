use super::fragments::insert_context_fragments;
use crate::conversation::{ResponseItem, input_items_to_plain_text};
use crate::text_input_items;

#[test]
fn standard_inserts_before_latest_real_user_not_summary() {
    let messages = vec![
        ResponseItem::System {
            content: "system".to_string(),
        },
        ResponseItem::User {
            content: text_input_items("latest user"),
        },
        ResponseItem::User {
            content: text_input_items("[Context Summary]\nsummary"),
        },
    ];
    let fragments = vec![ResponseItem::User {
        content: text_input_items("<environment_context>\nctx"),
    }];

    let injected = insert_context_fragments(messages, &fragments);

    assert!(matches!(
        &injected[..],
        [
            ResponseItem::System { .. },
            ResponseItem::User { content: env },
            ResponseItem::User { content: latest_user },
            ResponseItem::User { content: summary },
        ] if input_items_to_plain_text(env).starts_with("<environment_context>")
            && input_items_to_plain_text(latest_user) == "latest user"
            && input_items_to_plain_text(summary).starts_with("[Context Summary]")
    ));
}

#[test]
fn mid_turn_compaction_inserts_before_summary_when_tail_has_no_real_user() {
    let messages = vec![
        ResponseItem::System {
            content: "system".to_string(),
        },
        ResponseItem::User {
            content: text_input_items("[Context Summary]\nsummary"),
        },
        ResponseItem::Assistant {
            content: Some("assistant tail".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
        },
    ];
    let fragments = vec![ResponseItem::User {
        content: text_input_items("<environment_context>\nctx"),
    }];

    let injected = insert_context_fragments(messages, &fragments);

    assert!(matches!(
        &injected[..],
        [
            ResponseItem::System { content: system },
            ResponseItem::User { content: env },
            ResponseItem::User { content: summary },
            ResponseItem::Assistant { content: Some(assistant_tail), .. },
        ] if system == "system"
            && input_items_to_plain_text(env).starts_with("<environment_context>")
            && input_items_to_plain_text(summary) == "[Context Summary]\nsummary"
            && assistant_tail == "assistant tail"
    ));
}
