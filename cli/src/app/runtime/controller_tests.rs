use super::controller::{coalesce_client_events, should_stop_after_event_boundary};
use agent_app_server_client::AppServerEvent;
use agent_core::{
    RuntimeItem, RuntimeItemProgress, StructuredToolResult, TurnItemKind, WebSearchAction,
};
use agent_protocol::{AppServerMessage, AppServerNotification};

fn command_delta(item_id: &str, delta: &str) -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::CommandExecutionOutputDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: item_id.to_string(),
            call_id: Some("call-1".to_string()),
            delta: delta.to_string(),
        },
    ))
}

fn web_search_started() -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::ItemStarted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: RuntimeItem::started(
                "ws-1",
                Some("ws-1".to_string()),
                TurnItemKind::ToolResult,
                Some("web_search".to_string()),
            ),
        },
    ))
}

fn web_search_progress(query: &str) -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::ItemProgress {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "ws-1".to_string(),
            call_id: Some("ws-1".to_string()),
            progress: RuntimeItemProgress::message(query),
        },
    ))
}

fn json_patch_delta(item_id: &str, delta: &str) -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::JsonPatchDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: item_id.to_string(),
            call_id: Some("call-edit".to_string()),
            delta: delta.to_string(),
        },
    ))
}

fn web_search_completed(query: &str) -> AppServerEvent {
    let transcript_item = agent_core::TranscriptItem::ToolResult {
        id: "ws-1".to_string(),
        tool_name: "web_search".to_string(),
        content: query.to_string(),
        summary: "searched the web".to_string(),
        structured: Some(StructuredToolResult::WebSearch {
            query: query.to_string(),
            action: Some(WebSearchAction::Search {
                query: Some(query.to_string()),
                queries: None,
            }),
            result_count: None,
            source_count: None,
        }),
    };
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::ItemCompleted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: RuntimeItem::completed(&transcript_item, Some("ws-1".to_string())),
            transcript_item,
        },
    ))
}

fn command_started() -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::ItemStarted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: RuntimeItem::started(
                "tool:1",
                Some("call-1".to_string()),
                TurnItemKind::CommandExecution,
                Some("pwd".to_string()),
            ),
        },
    ))
}

#[test]
fn coalesces_adjacent_command_output_deltas_for_same_item() {
    let events = vec![
        command_delta("tool:1", "hello "),
        command_delta("tool:1", "world"),
    ];

    let coalesced = coalesce_client_events(events);

    assert_eq!(coalesced.len(), 1);
    let AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::CommandExecutionOutputDelta { delta, .. },
    )) = &coalesced[0]
    else {
        panic!("expected merged command delta");
    };
    assert_eq!(delta, "hello world");
}

#[test]
fn does_not_coalesce_command_output_across_items() {
    let events = vec![
        command_delta("tool:1", "hello "),
        command_delta("tool:2", "world"),
    ];

    let coalesced = coalesce_client_events(events);

    assert_eq!(coalesced.len(), 2);
}

#[test]
fn drops_lagged_markers_from_user_visible_event_stream() {
    let events = vec![
        AppServerEvent::Lagged { skipped: 3 },
        command_delta("tool:1", "done"),
    ];

    let coalesced = coalesce_client_events(events);

    assert_eq!(coalesced.len(), 1);
    assert!(matches!(coalesced[0], AppServerEvent::Message(_)));
}

#[test]
fn web_search_started_is_a_runtime_render_boundary() {
    assert!(should_stop_after_event_boundary(
        Some(&web_search_started())
    ));
}

#[test]
fn command_started_is_a_runtime_render_boundary() {
    assert!(should_stop_after_event_boundary(Some(&command_started())));
}

#[test]
fn web_search_progress_is_a_runtime_render_boundary() {
    assert!(should_stop_after_event_boundary(Some(
        &web_search_progress("weather seattle")
    )));
}

#[test]
fn web_search_completed_is_a_runtime_render_boundary() {
    assert!(should_stop_after_event_boundary(Some(
        &web_search_completed("weather seattle")
    )));
}

#[test]
fn json_patch_delta_is_a_runtime_render_boundary() {
    assert!(should_stop_after_event_boundary(Some(&json_patch_delta(
        "edit-1",
        "*** Begin Patch"
    ))));
}

#[test]
fn coalesces_adjacent_json_patch_deltas_for_same_item() {
    let events = vec![
        json_patch_delta("edit-1", "*** Begin Patch"),
        json_patch_delta("edit-1", "*** Update File: src/lib.rs"),
    ];

    let coalesced = coalesce_client_events(events);

    assert_eq!(coalesced.len(), 1);
    let AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::JsonPatchDelta { delta, .. },
    )) = &coalesced[0]
    else {
        panic!("expected merged json patch delta");
    };
    assert_eq!(delta, "*** Begin Patch\n*** Update File: src/lib.rs");
}
