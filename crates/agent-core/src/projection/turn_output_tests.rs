use super::*;
use crate::runtime_item::RuntimeItem;
use crate::tool::StructuredToolResult;
use crate::turn::TurnItemDeltaKind;

fn started(
    turn_id: &str,
    item_id: &str,
    call_id: Option<&str>,
    kind: crate::turn::TurnItemKind,
    title: Option<&str>,
) -> EventMsg {
    EventMsg::ItemStarted {
        turn_id: turn_id.to_string(),
        item: RuntimeItem::started(
            item_id,
            call_id.map(str::to_string),
            kind,
            title.map(str::to_string),
        ),
    }
}

fn completed(turn_id: &str, item: TranscriptItem, call_id: Option<&str>) -> EventMsg {
    let runtime_item = RuntimeItem::completed(&item, call_id.map(str::to_string));
    EventMsg::ItemCompleted {
        turn_id: turn_id.to_string(),
        runtime_item,
        transcript_item: item,
    }
}

#[test]
fn tool_event_uses_completed_item_not_streamed_delta() {
    let events = vec![
        started(
            "turn-1",
            "tool-1",
            Some("call-1"),
            crate::turn::TurnItemKind::CommandExecution,
            Some("exec_command"),
        ),
        EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "streamed stdout".to_string(),
        },
        completed(
            "turn-1",
            TranscriptItem::CommandExecution {
                id: "tool-1".to_string(),
                tool_name: "exec_command".to_string(),
                command: "pwd".to_string(),
                current_directory: "D:\\work".to_string(),
                status: crate::tool::CommandExecutionStatus::Completed,
                exit_code: Some(0),
                output: Some("D:\\work".to_string()),
                duration_ms: Some(1),
                summary: "completed summary".to_string(),
            },
            Some("call-1"),
        ),
    ];

    let tool_events = tool_events_from_turn_events(&events);

    assert_eq!(tool_events.len(), 1);
    assert_eq!(tool_events[0].name, "exec_command");
    assert_eq!(tool_events[0].summary, "completed summary");
    assert!(!tool_events[0].is_error);
}

#[test]
fn completed_tool_item_projects_tool_event() {
    let events = vec![
        started(
            "turn-1",
            "tool-1",
            Some("call-1"),
            crate::turn::TurnItemKind::ToolCall,
            Some("get_metadata"),
        ),
        completed(
            "turn-1",
            TranscriptItem::ToolResult {
                id: "tool-1".to_string(),
                tool_name: "get_metadata".to_string(),
                content: "ok".to_string(),
                summary: "ok".to_string(),
                structured: Some(StructuredToolResult::GetMetadata {
                    path: "Cargo.toml".to_string(),
                    exists: true,
                    is_file: true,
                    is_dir: false,
                    is_symlink: false,
                    size: 128,
                    readonly: false,
                    created_at_ms: None,
                    modified_at_ms: None,
                }),
            },
            Some("call-1"),
        ),
    ];

    let tool_events = tool_events_from_turn_events(&events);

    assert_eq!(tool_events.len(), 1);
    assert_eq!(tool_events[0].name, "get_metadata");
    assert_eq!(tool_events[0].summary, "ok");
    assert!(!tool_events[0].is_error);
}

#[test]
fn structured_tool_error_marks_event_as_error_without_summary_guessing() {
    let events = vec![completed(
        "turn-1",
        TranscriptItem::ToolResult {
            id: "tool-2".to_string(),
            tool_name: "tool_search".to_string(),
            content: "tool search failed".to_string(),
            summary: "tool search completed".to_string(),
            structured: Some(StructuredToolResult::ToolError {
                tool_name: "tool_search".to_string(),
                message: "not registered".to_string(),
            }),
        },
        Some("call-1"),
    )];

    let tool_events = tool_events_from_turn_events(&events);

    assert_eq!(tool_events.len(), 1);
    assert_eq!(tool_events[0].summary, "tool search completed");
    assert!(tool_events[0].is_error);
}
