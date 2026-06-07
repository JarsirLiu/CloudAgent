use super::*;
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
