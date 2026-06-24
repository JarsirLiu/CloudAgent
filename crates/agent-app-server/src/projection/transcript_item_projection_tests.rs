use super::transcript_item_projection::{
    projected_item_from_transcript_item, projected_item_to_transcript_item,
    projected_transcript_item_is_empty,
};
use crate::projection::turn_projection_state::ProjectedItemStatus;
use agent_core::TurnItemKind;
use agent_core::conversation::TranscriptItem;
use agent_core::tool::StructuredToolResult;

#[test]
fn command_execution_projection_preserves_command_identity_and_summary() {
    let item = TranscriptItem::CommandExecution {
        id: "cmd-1".to_string(),
        tool_name: "exec_command".to_string(),
        command: "git status".to_string(),
        current_directory: "D:\\work".to_string(),
        status: agent_core::CommandExecutionStatus::Completed,
        exit_code: Some(0),
        output: Some("clean".to_string()),
        duration_ms: Some(42),
        summary: "clean".to_string(),
    };

    let projected = projected_item_from_transcript_item("turn-1".to_string(), item, 7);

    assert_eq!(projected.kind, TurnItemKind::CommandExecution);
    assert_eq!(projected.title.as_deref(), Some("git status"));
    assert_eq!(projected.summary.as_deref(), Some("clean"));
    assert_eq!(projected.status, ProjectedItemStatus::Completed);
    assert_eq!(projected.tool_output_buffer, "clean");
}

#[test]
fn tool_result_projection_keeps_structured_result_roundtrip() {
    let item = TranscriptItem::ToolResult {
        id: "tool-1".to_string(),
        tool_name: "web_search".to_string(),
        content: "searched the web".to_string(),
        summary: "searched 2 sources".to_string(),
        structured: Some(StructuredToolResult::WebSearch {
            query: "weather seattle".to_string(),
            action: None,
            result_count: Some(2),
            source_count: Some(2),
        }),
    };

    let projected = projected_item_from_transcript_item("turn-1".to_string(), item.clone(), 9);
    let restored = projected_item_to_transcript_item(&projected).expect("restored item");

    assert_eq!(projected.kind, TurnItemKind::ToolResult);
    assert_eq!(projected.title.as_deref(), Some("web_search"));
    assert!(matches!(
        projected.structured,
        Some(StructuredToolResult::WebSearch {
            query,
            result_count,
            source_count,
            ..
        }) if query == "weather seattle"
            && result_count == Some(2)
            && source_count == Some(2)
    ));
    match restored {
        TranscriptItem::ToolResult {
            id,
            tool_name,
            content,
            summary,
            structured,
        } => {
            assert_eq!(id, "tool-1");
            assert_eq!(tool_name, "web_search");
            assert_eq!(content, "searched the web");
            assert_eq!(summary, "searched 2 sources");
            assert!(matches!(
                structured,
                Some(StructuredToolResult::WebSearch {
                    query,
                    result_count,
                    source_count,
                    ..
                }) if query == "weather seattle"
                    && result_count == Some(2)
                    && source_count == Some(2)
            ));
        }
        other => panic!("unexpected restored item: {other:?}"),
    }
}

#[test]
fn empty_transcript_items_are_filtered_by_summary_and_text_content() {
    let empty_tool_result = TranscriptItem::ToolResult {
        id: "tool-2".to_string(),
        tool_name: "web_search".to_string(),
        content: String::new(),
        summary: "   ".to_string(),
        structured: None,
    };
    let non_empty_tool_result = TranscriptItem::ToolResult {
        id: "tool-3".to_string(),
        tool_name: "web_search".to_string(),
        content: String::new(),
        summary: "searched".to_string(),
        structured: None,
    };

    assert!(projected_transcript_item_is_empty(&empty_tool_result));
    assert!(!projected_transcript_item_is_empty(&non_empty_tool_result));
}
