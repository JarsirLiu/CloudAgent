use super::*;
use agent_core::{
    CommandApprovalRequest, CommandExecutionStatus, RuntimeItem, ServerRequest, TranscriptItem,
    TurnItemKind,
};
use agent_protocol::{AppServerNotification, RequestId};

fn info_event(message: &str) -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::Info {
            conversation_id: "default".to_string(),
            message: message.to_string(),
        },
    ))
}

fn text_delta_event(delta: &str) -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::AgentMessageDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            delta: delta.to_string(),
        },
    ))
}

fn command_output_event(delta: &str) -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::CommandExecutionOutputDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool:1".to_string(),
            call_id: Some("call-1".to_string()),
            delta: delta.to_string(),
        },
    ))
}

fn item_started_event() -> AppServerEvent {
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

fn item_completed_event() -> AppServerEvent {
    let transcript_item = TranscriptItem::CommandExecution {
        id: "tool:1".to_string(),
        tool_name: "exec_command".to_string(),
        command: "pwd".to_string(),
        current_directory: "D:\\work".to_string(),
        status: CommandExecutionStatus::Completed,
        exit_code: Some(0),
        output: Some("D:\\work".to_string()),
        duration_ms: Some(1),
        summary: "current directory is D:\\work".to_string(),
    };
    AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::ItemCompleted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: RuntimeItem::completed(&transcript_item, Some("call-1".to_string())),
            transcript_item,
        },
    ))
}

fn server_request_event() -> AppServerEvent {
    AppServerEvent::Message(AppServerMessage::Request(
        agent_protocol::AppServerRequest::ServerRequest {
            request_id: RequestId::Integer(1),
            conversation_id: "default".to_string(),
            request: ServerRequest::CommandApproval {
                request: CommandApprovalRequest {
                    turn_id: "turn-1".to_string(),
                    tool_call_id: "call-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    reason: "need approval".to_string(),
                    command_preview: "{\"command\":\"pwd\"}".to_string(),
                },
            },
        },
    ))
}

#[tokio::test]
async fn non_critical_events_drop_when_queue_is_full() {
    let (tx, mut rx) = mpsc::channel(1);
    tx.send(info_event("already queued"))
        .await
        .expect("seed queue");
    let mut skipped = 0usize;

    assert!(forward_event(&tx, &mut skipped, info_event("drop me")).await);
    assert_eq!(skipped, 1);

    let first = rx.recv().await.expect("seed event");
    match first {
        AppServerEvent::Message(AppServerMessage::Notification(AppServerNotification::Info {
            message,
            ..
        })) => assert_eq!(message, "already queued"),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test]
async fn lossless_events_flush_lag_marker_before_delivery() {
    let (tx, mut rx) = mpsc::channel(1);
    tx.send(info_event("already queued"))
        .await
        .expect("seed queue");
    let mut skipped = 0usize;

    assert!(forward_event(&tx, &mut skipped, info_event("drop me")).await);
    assert_eq!(skipped, 1);

    let sender = tokio::spawn(async move {
        let mut skipped = skipped;
        let delivered = forward_event(&tx, &mut skipped, item_completed_event()).await;
        (delivered, skipped)
    });

    let first = rx.recv().await.expect("first event");
    let second = rx.recv().await.expect("second event");
    let third = rx.recv().await.expect("third event");
    let (delivered, skipped) = sender.await.expect("sender task");
    assert!(delivered);
    assert_eq!(skipped, 0);

    match first {
        AppServerEvent::Message(AppServerMessage::Notification(AppServerNotification::Info {
            message,
            ..
        })) => assert_eq!(message, "already queued"),
        other => panic!("unexpected first event: {other:?}"),
    }
    match second {
        AppServerEvent::Lagged { skipped } => assert_eq!(skipped, 1),
        other => panic!("unexpected second event: {other:?}"),
    }
    match third {
        AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::ItemCompleted { .. },
        )) => {}
        other => panic!("unexpected third event: {other:?}"),
    }
}

#[tokio::test]
async fn request_and_transcript_events_are_classified_lossless() {
    assert!(event_requires_delivery(&item_started_event()));
    assert!(event_requires_delivery(&item_completed_event()));
    assert!(event_requires_delivery(&text_delta_event("hello")));
    assert!(event_requires_delivery(&server_request_event()));
    assert!(!event_requires_delivery(&command_output_event("D:\\work")));
    assert!(!event_requires_delivery(&info_event("cosmetic")));
    assert!(!event_requires_delivery(&AppServerEvent::Lagged {
        skipped: 1
    }));
    assert!(!event_requires_delivery(&AppServerEvent::Disconnected {
        message: "bye".to_string()
    }));
}
