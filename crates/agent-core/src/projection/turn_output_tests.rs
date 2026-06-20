use super::*;
use crate::tool::StructuredToolResult;
use crate::turn::TurnItemDeltaKind;

#[test]
fn tool_event_uses_completed_item_not_streamed_delta() {
    let events = vec![
        EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: crate::turn::TurnItemKind::CommandExecution,
            title: Some("exec_command".to_string()),
        },
        EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: TurnItemDeltaKind::CommandExecutionOutput,
            segment_index: None,
            delta: "streamed stdout".to_string(),
        },
        EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            item: TranscriptItem::CommandExecution {
                id: "tool-1".to_string(),
                tool_name: "exec_command".to_string(),
                command: "pwd".to_string(),
                current_directory: "D:\\work".to_string(),
                status: CommandExecutionStatus::Completed,
                exit_code: Some(0),
                output: Some("D:\\work".to_string()),
                duration_ms: Some(1),
                summary: "completed summary".to_string(),
            },
        },
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
        EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            kind: crate::turn::TurnItemKind::ToolCall,
            title: Some("get_metadata".to_string()),
        },
        EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            item: TranscriptItem::ToolResult {
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
        },
    ];

    let tool_events = tool_events_from_turn_events(&events);

    assert_eq!(tool_events.len(), 1);
    assert_eq!(tool_events[0].name, "get_metadata");
    assert_eq!(tool_events[0].summary, "ok");
    assert!(!tool_events[0].is_error);
}
