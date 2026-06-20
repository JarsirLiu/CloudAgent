use crate::conversation::ResponseItem;
use crate::rollout::RolloutItem;
use crate::rollout::reconstruction::conversation_history_from_rollout_items;
use crate::tool::{ToolCall, ToolIdentity};
use crate::turn::{CompactionPhase, CompactionReason, CompactionTrigger, EventMsg};
use crate::{AttachmentRef, ImageDetail, InputItem, input_items_to_plain_text, text_input_items};
use serde_json::json;

fn summary() -> crate::context::CompactionSummary {
    crate::context::CompactionSummary::from_model_output("Current Task:\n- old").ensure_defaults()
}

#[test]
fn rebuilds_model_messages_from_rollout_response_items() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system prompt",
        &[
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("hi"),
            }),
            RolloutItem::from(ResponseItem::Assistant {
                content: Some("hello".to_string()),
                reasoning: Some("hidden chain".to_string()),
                tool_calls: Vec::new(),
            }),
        ],
    );

    assert_eq!(history.id, "default");
    assert_eq!(history.turn_count, 1);
    assert!(matches!(
        &history.messages[..],
        [
            ResponseItem::System { content: system },
            ResponseItem::User { content: user },
            ResponseItem::Assistant {
                content: Some(assistant),
                reasoning: Some(reasoning),
                ..
            },
        ] if system == "system prompt"
            && input_items_to_plain_text(user) == "hi"
            && assistant == "hello"
            && reasoning == "hidden chain"
    ));
}

#[test]
fn preserves_local_image_paths_from_rollout() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system prompt",
        &[RolloutItem::from(ResponseItem::User {
            content: vec![InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: "D:\\images\\diagram.png".to_string(),
                },
                detail: Some(ImageDetail::High),
                alt: Some("diagram".to_string()),
            }],
        })],
    );

    assert!(matches!(
        &history.messages[..],
        [
            ResponseItem::System { .. },
            ResponseItem::User { content },
        ] if matches!(
            &content[..],
            [InputItem::Image {
                source: AttachmentRef::LocalPath { path },
                detail: Some(ImageDetail::High),
                alt: Some(alt),
            }] if path == "D:\\images\\diagram.png" && alt == "diagram"
        )
    ));
}

#[test]
fn prefers_compacted_replacement_history() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system prompt",
        &[
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("old"),
            }),
            RolloutItem::Compacted {
                summary: summary(),
                rendered_summary: "[Context Summary]\nold".to_string(),
                trigger: CompactionTrigger::Auto,
                reason: CompactionReason::ContextLimit,
                phase: CompactionPhase::PreTurn,
                replacement_history: vec![
                    ResponseItem::System {
                        content: "system prompt".to_string(),
                    },
                    ResponseItem::User {
                        content: text_input_items("[Context Summary]\nold"),
                    },
                    ResponseItem::User {
                        content: text_input_items("latest"),
                    },
                    ResponseItem::Assistant {
                        content: Some("current".to_string()),
                        reasoning: None,
                        tool_calls: Vec::new(),
                    },
                ],
            },
        ],
    );

    assert_eq!(history.turn_count, 1);
    assert!(matches!(
        &history.messages[..],
        [
            ResponseItem::System { content: system },
            ResponseItem::User { content: summary },
            ResponseItem::User { content: user },
            ResponseItem::Assistant {
                content: Some(assistant),
                ..
            },
        ] if system == "system prompt"
            && input_items_to_plain_text(summary) == "[Context Summary]\nold"
            && input_items_to_plain_text(user) == "latest"
            && assistant == "current"
    ));
}

#[test]
fn normalizes_legacy_compacted_summary_system_message() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system prompt",
        &[RolloutItem::Compacted {
            summary: summary(),
            rendered_summary: "[Context Summary]\nold".to_string(),
            trigger: CompactionTrigger::Auto,
            reason: CompactionReason::ContextLimit,
            phase: CompactionPhase::PreTurn,
            replacement_history: vec![
                ResponseItem::System {
                    content: "system prompt".to_string(),
                },
                ResponseItem::System {
                    content: "[Context Summary]\nold".to_string(),
                },
                ResponseItem::User {
                    content: text_input_items("latest"),
                },
            ],
        }],
    );

    assert_eq!(history.turn_count, 1);
    assert!(matches!(
        &history.messages[..],
        [
            ResponseItem::System { content: system },
            ResponseItem::User { content: summary },
            ResponseItem::User { content: latest },
        ] if system == "system prompt"
            && input_items_to_plain_text(summary) == "[Context Summary]\nold"
            && input_items_to_plain_text(latest) == "latest"
    ));
}

#[test]
fn keeps_post_compaction_items_in_same_turn_suffix() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system prompt",
        &[
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("old"),
            }),
            RolloutItem::Compacted {
                summary: summary(),
                rendered_summary: "[Context Summary]\nold".to_string(),
                trigger: CompactionTrigger::Auto,
                reason: CompactionReason::ContextLimit,
                phase: CompactionPhase::MidTurn,
                replacement_history: vec![
                    ResponseItem::System {
                        content: "system prompt".to_string(),
                    },
                    ResponseItem::User {
                        content: text_input_items("[Context Summary]\nold"),
                    },
                    ResponseItem::User {
                        content: text_input_items("latest"),
                    },
                ],
            },
            RolloutItem::from(ResponseItem::Assistant {
                content: Some("after compact assistant".to_string()),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    identity: ToolIdentity::built_in("read_file"),
                    arguments: json!({"path": "README.md"}),
                }],
            }),
            RolloutItem::from(ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "read_file".to_string(),
                content: "file body".to_string(),
                structured: None,
            }),
        ],
    );

    assert!(matches!(
        &history.messages[..],
        [
            ResponseItem::System { content: system },
            ResponseItem::User { content: summary },
            ResponseItem::User { content: latest_user },
            ResponseItem::Assistant {
                content: Some(assistant),
                tool_calls,
                ..
            },
            ResponseItem::Tool { tool_call_id, content, .. },
        ] if system == "system prompt"
            && input_items_to_plain_text(summary) == "[Context Summary]\nold"
            && input_items_to_plain_text(latest_user) == "latest"
            && assistant == "after compact assistant"
            && tool_calls.len() == 1
            && tool_calls[0].id == "call-1"
            && tool_call_id == "call-1"
            && content == "file body"
    ));
}

#[test]
fn rebuild_starts_from_latest_compaction_checkpoint() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system prompt",
        &[
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("old before checkpoint"),
            }),
            RolloutItem::Compacted {
                summary: summary(),
                rendered_summary: "[Context Summary]\nfirst".to_string(),
                trigger: CompactionTrigger::Auto,
                reason: CompactionReason::ContextLimit,
                phase: CompactionPhase::PreTurn,
                replacement_history: vec![
                    ResponseItem::System {
                        content: "system prompt".to_string(),
                    },
                    ResponseItem::User {
                        content: text_input_items("first checkpoint user"),
                    },
                    ResponseItem::User {
                        content: text_input_items("[Context Summary]\nfirst"),
                    },
                ],
            },
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("between checkpoints"),
            }),
            RolloutItem::Compacted {
                summary: summary(),
                rendered_summary: "[Context Summary]\nsecond".to_string(),
                trigger: CompactionTrigger::Auto,
                reason: CompactionReason::ContextLimit,
                phase: CompactionPhase::MidTurn,
                replacement_history: vec![
                    ResponseItem::System {
                        content: "system prompt".to_string(),
                    },
                    ResponseItem::User {
                        content: text_input_items("second checkpoint user"),
                    },
                    ResponseItem::User {
                        content: text_input_items("[Context Summary]\nsecond"),
                    },
                ],
            },
            RolloutItem::from(ResponseItem::Assistant {
                content: Some("suffix assistant".to_string()),
                reasoning: None,
                tool_calls: Vec::new(),
            }),
        ],
    );

    let rendered = history
        .messages
        .iter()
        .map(|item| match item {
            ResponseItem::System { content } => content.clone(),
            ResponseItem::User { content } => input_items_to_plain_text(content),
            ResponseItem::Assistant { content, .. } => content.clone().unwrap_or_default(),
            ResponseItem::Tool { content, .. } => content.clone(),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        rendered,
        vec![
            "system prompt".to_string(),
            "second checkpoint user".to_string(),
            "[Context Summary]\nsecond".to_string(),
            "suffix assistant".to_string(),
        ]
    );
}

#[test]
fn drops_cancelled_user_only_turns() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system",
        &[
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("hi"),
            }),
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: text_input_items("hi"),
            }),
            RolloutItem::from(ResponseItem::Assistant {
                content: Some("hello".to_string()),
                reasoning: None,
                tool_calls: Vec::new(),
            }),
            RolloutItem::from(EventMsg::TurnCompleted {
                turn_id: "turn-1".to_string(),
            }),
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("continue"),
            }),
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-2".to_string(),
                conversation_id: "default".to_string(),
                user_input: text_input_items("continue"),
            }),
            RolloutItem::from(EventMsg::TurnCancelled {
                turn_id: "turn-2".to_string(),
                reason: "interrupted by client".to_string(),
            }),
        ],
    );

    assert_eq!(history.turn_count, 1);
    assert_eq!(history.messages.len(), 3);
    assert!(matches!(
        &history.messages[..],
        [
            ResponseItem::System { .. },
            ResponseItem::User { content },
            ResponseItem::Assistant { content: Some(answer), .. }
        ] if input_items_to_plain_text(content) == "hi" && answer == "hello"
    ));
}

#[test]
fn keeps_cancelled_turn_with_model_output() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system",
        &[
            RolloutItem::from(ResponseItem::User {
                content: text_input_items("hi"),
            }),
            RolloutItem::from(EventMsg::TurnStarted {
                turn_id: "turn-1".to_string(),
                conversation_id: "default".to_string(),
                user_input: text_input_items("hi"),
            }),
            RolloutItem::from(ResponseItem::Assistant {
                content: Some("partial".to_string()),
                reasoning: None,
                tool_calls: Vec::new(),
            }),
            RolloutItem::from(EventMsg::TurnCancelled {
                turn_id: "turn-1".to_string(),
                reason: "interrupted by client".to_string(),
            }),
        ],
    );

    assert_eq!(history.turn_count, 1);
    assert!(history.messages.iter().any(|item| {
        matches!(
            item,
            ResponseItem::Assistant { content: Some(answer), .. } if answer == "partial"
        )
    }));
    assert!(matches!(
        history.messages.last(),
        Some(ResponseItem::User { content }) if input_items_to_plain_text(content).contains("<turn_aborted>")
    ));
}

#[test]
fn marks_cancelled_compacted_tool_turn_as_aborted() {
    let history = conversation_history_from_rollout_items(
        "default",
        "system",
        &[
            RolloutItem::Compacted {
                summary: crate::context::CompactionSummary::from_model_output(
                    "Current Task:\n- older",
                )
                .ensure_defaults(),
                rendered_summary: "[Context Summary]\nolder".to_string(),
                trigger: CompactionTrigger::Auto,
                reason: CompactionReason::ContextLimit,
                phase: CompactionPhase::PreTurn,
                replacement_history: vec![
                    ResponseItem::System {
                        content: "system".to_string(),
                    },
                    ResponseItem::User {
                        content: text_input_items("[Context Summary]\nolder"),
                    },
                    ResponseItem::User {
                        content: text_input_items("continue work"),
                    },
                ],
            },
            RolloutItem::from(ResponseItem::Assistant {
                content: None,
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "exec_command".to_string(),
                    identity: ToolIdentity::built_in("exec_command"),
                    arguments: json!({"command":"pwd"}),
                }],
            }),
            RolloutItem::from(ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "exec_command".to_string(),
                content: "D:\\work".to_string(),
                structured: None,
            }),
            RolloutItem::from(EventMsg::TurnCancelled {
                turn_id: "turn-1".to_string(),
                reason: "interrupted by client".to_string(),
            }),
        ],
    );

    assert!(history.messages.iter().any(|item| {
        matches!(
            item,
            ResponseItem::Tool { tool_call_id, .. } if tool_call_id == "call-1"
        )
    }));
    assert!(matches!(
        history.messages.last(),
        Some(ResponseItem::User { content }) if input_items_to_plain_text(content).contains("<turn_aborted>")
    ));
}
