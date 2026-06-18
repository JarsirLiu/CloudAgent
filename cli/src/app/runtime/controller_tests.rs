use super::controller::coalesce_client_events;
use agent_app_server_client::AppServerEvent;
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
