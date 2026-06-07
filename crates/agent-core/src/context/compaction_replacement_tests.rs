use super::{CompactionSummary, ContextCompactionPlan, build_compacted_replacement_history};
use crate::conversation::{ResponseItem, input_items_to_plain_text};
use crate::text_input_items;
use crate::tool::{ToolCall, ToolIdentity};
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
