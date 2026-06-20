use super::plan::{adjust_tail_start_for_tool_invariants, choose_tail_start};
use super::*;
use crate::conversation::{ResponseItem, input_items_to_plain_text};
use crate::tool::{CommandExecutionStatus, StructuredToolResult, ToolCall, ToolIdentity};
use crate::{AttachmentRef, ImageDetail, InputItem, text_input_items};
use serde_json::json;

fn summary() -> CompactionSummary {
    CompactionSummary::from_model_output(
        "Current Task:\n- Continue\n\nProgress:\n- Tool output was summarized\n\nKey Decisions:\n- Keep replacement history clean\n\nImportant Context:\n- Use recent user intent\n\nTool / Code Facts:\n- exec_command returned ok\n\nNext Steps:\n- Continue",
    )
    .ensure_defaults()
}

#[test]
fn replacement_keeps_only_system_real_users_and_summary() {
    let plan = ContextCompactionPlan {
        prefix: Vec::new(),
        preserved_tail: vec![
            ResponseItem::Assistant {
                content: Some("raw assistant should not stay".to_string()),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "exec_command".to_string(),
                    identity: ToolIdentity::built_in("exec_command"),
                    arguments: json!({"command": "pwd"}),
                }],
            },
            ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "exec_command".to_string(),
                content: "raw tool should not stay".to_string(),
                structured: None,
            },
            ResponseItem::User {
                content: text_input_items("latest real user"),
            },
            ResponseItem::User {
                content: text_input_items("[Context Summary]\nlegacy"),
            },
        ],
    };
    let result = build_compacted_replacement_history(
        &[ResponseItem::System {
            content: "system".to_string(),
        }],
        &plan,
        &summary(),
    );

    assert!(matches!(
        &result.messages[..],
        [
            ResponseItem::System { content: system },
            ResponseItem::User { content: latest_user },
            ResponseItem::User { content: compacted_summary },
        ] if system == "system"
            && input_items_to_plain_text(latest_user) == "latest real user"
            && input_items_to_plain_text(compacted_summary).starts_with("[Context Summary]")
    ));
}

#[test]
fn plans_compaction_when_history_exceeds_trigger() {
    let mut messages = vec![ResponseItem::System {
        content: "system".to_string(),
    }];
    for i in 0..40 {
        messages.push(ResponseItem::User {
            content: text_input_items(format!("user line {i} {}", "x".repeat(50))),
        });
        messages.push(ResponseItem::Assistant {
            content: Some(format!("assistant line {i} {}", "y".repeat(50))),
            reasoning: None,
            tool_calls: Vec::new(),
        });
    }

    let plan = plan_history_compaction(
        &messages,
        ContextCompactionConfig {
            model_context_window: 2_048,
            trigger_ratio: 0.5,
            compacted_target_tokens: 720,
            preserved_user_turns: 3,
            preserved_tail_tokens: 512,
            summary_source_max_tokens: 600,
        },
    )
    .expect("should compact");

    let request = build_compaction_summary_request(
        &plan,
        ContextCompactionConfig {
            model_context_window: 2_048,
            trigger_ratio: 0.5,
            compacted_target_tokens: 720,
            preserved_user_turns: 3,
            preserved_tail_tokens: 512,
            summary_source_max_tokens: 600,
        },
        0.0,
    );
    assert_eq!(request.tools.len(), 0);
    assert!(matches!(request.messages[0], ResponseItem::System { .. }));
    assert!(matches!(request.messages[1], ResponseItem::User { .. }));
}

#[test]
fn applies_compaction_as_clean_checkpoint_history() {
    let mut messages = vec![ResponseItem::System {
        content: "system".to_string(),
    }];
    for i in 0..20 {
        messages.push(ResponseItem::User {
            content: text_input_items(format!("q{i} {}", "z".repeat(80))),
        });
        messages.push(ResponseItem::Assistant {
            content: Some(format!("a{i} {}", "w".repeat(80))),
            reasoning: None,
            tool_calls: vec![ToolCall {
                id: format!("call-{i}"),
                name: "exec_command".to_string(),
                identity: crate::tool::ToolIdentity::built_in("exec_command"),
                arguments: json!({"command":"echo test"}),
            }],
        });
        messages.push(ResponseItem::Tool {
            tool_call_id: format!("call-{i}"),
            name: "exec_command".to_string(),
            content: "ok".to_string(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: "echo test".to_string(),
                current_directory: "D:\\work".to_string(),
                session_id: None,
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                success: Some(true),
                output: Some("ok".to_string()),
                duration_ms: Some(1),
                original_token_count: Some(1),
                max_output_tokens: Some(10_000),
            }),
        });
    }
    let plan = plan_history_compaction(
        &messages,
        ContextCompactionConfig {
            model_context_window: 2_048,
            trigger_ratio: 0.45,
            compacted_target_tokens: 614,
            preserved_user_turns: 3,
            preserved_tail_tokens: 512,
            summary_source_max_tokens: 600,
        },
    )
    .expect("plan should exist");

    let result = apply_history_compaction(
        &mut messages,
        &plan,
        CompactionSummary::from_model_output(
            "Current Task:\n- Test\n\nProgress:\n- Done\n\nKey Decisions:\n- Keep core-owned compaction\n\nImportant Context:\n- Preserve system prompt\n\nTool / Code Facts:\n- exec_command used\n\nNext Steps:\n- Continue",
        )
        .ensure_defaults(),
    );

    assert!(messages.iter().all(|item| {
        matches!(
            item,
            ResponseItem::System { .. } | ResponseItem::User { .. }
        )
    }));
    assert!(messages.iter().any(|item| {
        matches!(item, ResponseItem::User { content } if input_items_to_plain_text(content).starts_with("q"))
    }));
    assert!(result.summary.rendered().contains("[Context Summary]"));
    assert_eq!(result.replacement_history.len(), messages.len());
    assert!(matches!(
        messages.last(),
        Some(ResponseItem::User { content }) if input_items_to_plain_text(content).starts_with("[Context Summary]")
    ));
}

#[test]
fn compaction_source_keeps_multimodal_user_item_details() {
    let source = super::summary::CompactionSummary::fallback_from_plan(&ContextCompactionPlan {
        prefix: vec![ResponseItem::User {
            content: vec![
                InputItem::Text {
                    text: "please inspect".to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::RemoteUrl {
                        url: "https://example.com/diagram.png".to_string(),
                    },
                    detail: Some(ImageDetail::High),
                    alt: Some("system diagram".to_string()),
                },
                InputItem::File {
                    source: AttachmentRef::HubAsset {
                        asset_id: "asset-1".to_string(),
                        download_url: None,
                    },
                    mime_type: Some("application/pdf".to_string()),
                    name: Some("spec.pdf".to_string()),
                },
                InputItem::Mention {
                    name: "browser-use".to_string(),
                    path: "plugin://browser-use".to_string(),
                },
            ],
        }],
        preserved_tail: Vec::new(),
    });

    assert!(source.current_task[0].contains("please inspect"));
    assert!(
        source.current_task[0].contains(
            "[image alt=system diagram detail=high source=https://example.com/diagram.png]"
        )
    );
    assert!(
        source.current_task[0]
            .contains("[file name=spec.pdf mime=application/pdf source=hub:asset-1]")
    );
    assert!(source.current_task[0].contains("[mention @browser-use path=plugin://browser-use]"));
}

#[test]
fn fallback_summary_uses_multimodal_user_rendering() {
    let plan = ContextCompactionPlan {
        prefix: vec![ResponseItem::User {
            content: vec![
                InputItem::Text {
                    text: "compare this".to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::LocalPath {
                        path: "D:\\images\\shot.png".to_string(),
                    },
                    detail: Some(ImageDetail::Low),
                    alt: Some("ui screenshot".to_string()),
                },
            ],
        }],
        preserved_tail: Vec::new(),
    };

    let summary = CompactionSummary::fallback_from_plan(&plan);

    assert!(summary.current_task[0].contains("compare this"));
    assert!(
        summary.current_task[0]
            .contains("[image alt=ui screenshot detail=low source=D:\\images\\shot.png]")
    );
}

#[test]
fn adjust_tail_start_includes_tool_call_for_preserved_tool_result() {
    let messages = vec![
        ResponseItem::System {
            content: "system".repeat(20),
        },
        ResponseItem::User {
            content: text_input_items(format!("first {}", "x".repeat(80))),
        },
        ResponseItem::Assistant {
            content: Some(format!("calling tool {}", "y".repeat(80))),
            reasoning: None,
            tool_calls: vec![ToolCall {
                id: "call-1".to_string(),
                name: "exec_command".to_string(),
                identity: crate::tool::ToolIdentity::built_in("exec_command"),
                arguments: json!({"command":"pwd"}),
            }],
        },
        ResponseItem::Tool {
            tool_call_id: "call-1".to_string(),
            name: "exec_command".to_string(),
            content: format!("D:/learn/gifti/cloudagent {}", "z".repeat(80)),
            structured: Some(StructuredToolResult::CommandExecution {
                command: "pwd".to_string(),
                current_directory: "D:/learn/gifti/cloudagent".to_string(),
                session_id: None,
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                success: Some(true),
                output: Some("D:/learn/gifti/cloudagent".to_string()),
                duration_ms: Some(1),
                original_token_count: Some(8),
                max_output_tokens: Some(10_000),
            }),
        },
        ResponseItem::User {
            content: text_input_items(format!("continue {}", "q".repeat(80))),
        },
        ResponseItem::Assistant {
            content: Some(format!("done {}", "w".repeat(80))),
            reasoning: None,
            tool_calls: Vec::new(),
        },
        ResponseItem::User {
            content: text_input_items(format!("follow up {}", "n".repeat(80))),
        },
    ];

    let adjusted = adjust_tail_start_for_tool_invariants(&messages, 3);
    assert_eq!(adjusted, 2);
}

#[test]
fn prefers_recent_user_boundary_when_tail_budget_allows() {
    let mut messages = vec![ResponseItem::System {
        content: "system".to_string(),
    }];
    for i in 0..6 {
        messages.push(ResponseItem::User {
            content: text_input_items(format!("user-{i} {}", "x".repeat(40))),
        });
        messages.push(ResponseItem::Assistant {
            content: Some(format!("assistant-{i} {}", "y".repeat(20))),
            reasoning: None,
            tool_calls: Vec::new(),
        });
    }

    let keep_start = choose_tail_start(&messages, 3, 200);
    assert!(matches!(
        &messages[keep_start],
        ResponseItem::User { content } if input_items_to_plain_text(content).starts_with("user-3")
    ));
}

#[test]
fn falls_back_to_smaller_recent_suffix_when_requested_user_count_exceeds_tail_budget() {
    let mut messages = vec![ResponseItem::System {
        content: "system".to_string(),
    }];
    for i in 0..4 {
        messages.push(ResponseItem::User {
            content: text_input_items(format!("user-{i} {}", "x".repeat(160))),
        });
        messages.push(ResponseItem::Assistant {
            content: Some(format!("assistant-{i} {}", "y".repeat(160))),
            reasoning: None,
            tool_calls: Vec::new(),
        });
    }

    let keep_start = choose_tail_start(&messages, 3, 120);
    assert!(keep_start > 1);
    assert!(super::support::estimate_message_tokens(&messages[keep_start..]) <= 120);
}
