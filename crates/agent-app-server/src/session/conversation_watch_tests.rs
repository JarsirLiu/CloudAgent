use super::*;
use agent_core::{
    RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress, RuntimeItemSnapshot, TurnItemKind,
    TurnState,
};
use agent_protocol::{
    AppServerMessage, ConversationActiveFlag, ConversationViewStatus, ServerRequestViewKind,
};
use tokio::time::{Duration, timeout};

const CONVERSATION_ID: &str = "default";

#[tokio::test]
async fn unknown_conversation_is_not_loaded() {
    let (manager, _rx) = test_manager();

    assert!(matches!(
        manager.snapshot(CONVERSATION_ID).await.status,
        ConversationViewStatus::NotLoaded
    ));
}

#[tokio::test]
async fn changed_status_emits_conversation_view_changed() {
    let (manager, mut rx) = test_manager();

    manager.note_turn_starting(CONVERSATION_ID).await;

    let snapshot = recv_conversation_view_changed(&mut rx).await;
    assert!(matches!(
        snapshot.status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::RunningTurn)
    ));
}

#[tokio::test]
async fn duplicate_request_resolved_does_not_emit_notification() {
    let (manager, mut rx) = test_manager();
    let request_id = RequestId::Integer(7);

    manager
        .note_server_request_pending(CONVERSATION_ID, pending_request(request_id.clone()))
        .await
        .release()
        .await;
    let _pending = recv_conversation_view_changed(&mut rx).await;
    let _resolved = recv_conversation_view_changed(&mut rx).await;

    manager
        .note_server_request_resolved(CONVERSATION_ID, request_id)
        .await;

    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "duplicate request resolution should not rebroadcast unchanged view"
    );
}

#[tokio::test]
async fn emit_current_replays_snapshot_even_when_unchanged() {
    let (manager, mut rx) = test_manager();

    manager.emit_current(CONVERSATION_ID).await;

    let snapshot = recv_conversation_view_changed(&mut rx).await;
    assert!(matches!(snapshot.status, ConversationViewStatus::NotLoaded));
}

#[tokio::test]
async fn approval_guard_drop_clears_waiting_on_approval() {
    let (manager, mut rx) = test_manager();
    manager
        .note_turn_started(CONVERSATION_ID, "turn-1".to_string())
        .await;
    let _started = recv_conversation_view_changed(&mut rx).await;

    let guard = manager
        .note_server_request_pending(CONVERSATION_ID, pending_request(RequestId::Integer(7)))
        .await;
    let pending = recv_conversation_view_changed(&mut rx).await;
    assert!(matches!(
        pending.status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::WaitingOnApproval)
    ));

    drop(guard);

    let resolved = recv_conversation_view_changed(&mut rx).await;
    assert!(matches!(
        resolved.status,
        ConversationViewStatus::Active { ref flags, .. }
            if !flags.contains(&ConversationActiveFlag::WaitingOnApproval)
    ));
}

#[tokio::test]
async fn watch_subscriber_receives_latest_snapshot() {
    let (manager, _rx) = test_manager();

    manager.note_turn_starting(CONVERSATION_ID).await;
    let watcher = manager.subscribe(CONVERSATION_ID).await;

    assert!(matches!(
        watcher.borrow().status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::RunningTurn)
    ));
}

#[tokio::test]
async fn user_input_guard_sets_and_clears_waiting_on_user_input() {
    let (manager, mut rx) = test_manager();
    manager
        .note_turn_started(CONVERSATION_ID, "turn-1".to_string())
        .await;
    let _started = recv_conversation_view_changed(&mut rx).await;

    let guard = manager.note_user_input_requested(CONVERSATION_ID).await;
    let pending = recv_conversation_view_changed(&mut rx).await;
    assert!(matches!(
        pending.status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::WaitingOnUserInput)
    ));

    drop(guard);

    let resolved = recv_conversation_view_changed(&mut rx).await;
    assert!(matches!(
        resolved.status,
        ConversationViewStatus::Active { ref flags, .. }
            if !flags.contains(&ConversationActiveFlag::WaitingOnUserInput)
    ));
}

#[tokio::test]
async fn runtime_item_snapshot_change_rebroadcasts_conversation_view() {
    let (manager, mut rx) = test_manager();
    manager
        .note_turn_started(CONVERSATION_ID, "turn-1".to_string())
        .await;
    let _started = recv_conversation_view_changed(&mut rx).await;

    manager
        .note_active_turn_snapshot(
            CONVERSATION_ID,
            Some(runtime_turn(runtime_item_snapshot(
                "ws-1",
                "first query",
                Some(2),
            ))),
        )
        .await;
    let initial = recv_conversation_view_changed(&mut rx).await;
    assert_eq!(
        initial
            .active_turn
            .as_ref()
            .expect("active turn")
            .runtime_items
            .len(),
        1
    );

    manager
        .note_active_turn_snapshot(
            CONVERSATION_ID,
            Some(runtime_turn(runtime_item_snapshot(
                "ws-1",
                "refined query",
                Some(4),
            ))),
        )
        .await;
    let updated = recv_conversation_view_changed(&mut rx).await;
    let runtime_item = &updated
        .active_turn
        .as_ref()
        .expect("active turn")
        .runtime_items[0];
    assert_eq!(
        runtime_item
            .item
            .progress
            .as_ref()
            .and_then(|p| p.message.as_deref()),
        Some("refined query")
    );
    assert_eq!(
        runtime_item
            .item
            .metrics
            .as_ref()
            .and_then(|m| m.result_count),
        Some(4)
    );
}

fn test_manager() -> (
    ConversationWatchManager,
    mpsc::UnboundedReceiver<AppServerMessage>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let state = Arc::new(Mutex::new(ServerState::new(
        CONVERSATION_ID.to_string(),
        true,
    )));
    (ConversationWatchManager::new(tx, state), rx)
}

async fn recv_conversation_view_changed(
    rx: &mut mpsc::UnboundedReceiver<AppServerMessage>,
) -> ConversationViewSnapshot {
    let message = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("timed out waiting for conversation view change")
        .expect("event channel should remain open");
    match message {
        AppServerMessage::Notification(AppServerNotification::ConversationViewChanged {
            snapshot,
            ..
        }) => snapshot,
        other => panic!("expected conversation view change, got {other:?}"),
    }
}

fn pending_request(request_id: RequestId) -> PendingServerRequestView {
    PendingServerRequestView {
        request_id,
        conversation_id: CONVERSATION_ID.to_string(),
        turn_id: "turn-1".to_string(),
        kind: ServerRequestViewKind::CommandApproval,
        tool_name: "exec_command".to_string(),
        reason: "needs approval".to_string(),
        preview: "git status".to_string(),
        created_at_ms: 42,
    }
}

fn runtime_turn(snapshot: RuntimeItemSnapshot) -> ConversationTurn {
    ConversationTurn {
        id: "turn-1".to_string(),
        state: TurnState::Running,
        items: Vec::new(),
        runtime_items: vec![snapshot],
        rollout_start_index: 0,
        rollout_end_index: 0,
    }
}

fn runtime_item_snapshot(
    item_id: &str,
    progress_message: &str,
    result_count: Option<usize>,
) -> RuntimeItemSnapshot {
    RuntimeItemSnapshot {
        item: RuntimeItem::started(
            item_id,
            Some("call-1".to_string()),
            TurnItemKind::ToolResult,
            Some("web_search".to_string()),
        )
        .with_progress(RuntimeItemProgress::message(progress_message))
        .with_metrics(RuntimeItemMetrics {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            elapsed_ms: None,
            bytes_read: None,
            bytes_written: None,
            file_count: None,
            source_count: Some(1),
            result_count,
        }),
        text_buffer: String::new(),
        reasoning_buffer: String::new(),
        tool_output_buffer: progress_message.to_string(),
        patch_buffer: String::new(),
    }
}
