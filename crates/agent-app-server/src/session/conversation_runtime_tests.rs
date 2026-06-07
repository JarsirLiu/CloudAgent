use super::*;
use agent_protocol::ServerRequestViewKind;

#[test]
fn unknown_conversation_is_not_loaded() {
    let manager = ConversationRuntimeViewManager::default();

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::NotLoaded
    ));
}

#[test]
fn loaded_conversation_is_idle() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.mark_loaded("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Idle
    ));
}

#[test]
fn turn_starting_projects_active_without_turn_id() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.turn_starting("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active {
            active_turn_id: None,
            ref flags,
        } if flags == &vec![ConversationActiveFlag::RunningTurn]
    ));
}

#[test]
fn turn_started_sets_real_turn_id() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.turn_started("default", "turn-1".to_string());

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active {
            active_turn_id: Some(ref turn_id),
            ref flags,
        } if turn_id == "turn-1" && flags.contains(&ConversationActiveFlag::RunningTurn)
    ));
}

#[test]
fn pending_request_sets_waiting_on_approval_until_resolved() {
    let mut manager = ConversationRuntimeViewManager::default();
    let request_id = RequestId::Integer(7);

    manager.turn_started("default", "turn-1".to_string());
    manager.request_pending("default", pending_request(request_id.clone()));

    let snapshot = manager.snapshot("default");
    assert_eq!(snapshot.pending_requests.len(), 1);
    assert!(matches!(
        snapshot.status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::WaitingOnApproval)
    ));

    manager.request_resolved("default", &request_id);

    let snapshot = manager.snapshot("default");
    assert!(snapshot.pending_requests.is_empty());
    assert!(matches!(
        snapshot.status,
        ConversationViewStatus::Active { ref flags, .. }
            if !flags.contains(&ConversationActiveFlag::WaitingOnApproval)
    ));
}

#[test]
fn resolving_missing_request_is_idempotent() {
    let mut manager = ConversationRuntimeViewManager::default();
    let request_id = RequestId::Integer(7);

    manager.turn_started("default", "turn-1".to_string());
    manager.request_pending("default", pending_request(request_id.clone()));
    manager.request_resolved("default", &request_id);
    let resolved_once = manager.snapshot("default");

    manager.request_resolved("default", &request_id);
    let resolved_twice = manager.snapshot("default");

    assert_eq!(resolved_twice.updated_at_ms, resolved_once.updated_at_ms);
    assert_eq!(
        resolved_twice.pending_requests.len(),
        resolved_once.pending_requests.len()
    );
    assert_eq!(resolved_twice.status, resolved_once.status);
}

#[test]
fn interrupt_and_compaction_flags_are_active_flags() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.turn_started("default", "turn-1".to_string());
    manager.interrupt_requested("default");
    manager.compaction_started("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::InterruptRequested)
                && flags.contains(&ConversationActiveFlag::CompactingContext)
    ));

    manager.compaction_finished("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active { ref flags, .. }
            if !flags.contains(&ConversationActiveFlag::CompactingContext)
    ));
}

#[test]
fn waiting_on_user_input_is_distinct_from_approval() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.turn_started("default", "turn-1".to_string());
    manager.user_input_requested("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::WaitingOnUserInput)
    ));

    manager.user_input_resolved("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active { ref flags, .. }
            if !flags.contains(&ConversationActiveFlag::WaitingOnUserInput)
    ));
}

#[test]
fn nested_user_input_requests_use_count() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.turn_started("default", "turn-1".to_string());
    manager.user_input_requested("default");
    manager.user_input_requested("default");
    manager.user_input_resolved("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::WaitingOnUserInput)
    ));

    manager.user_input_resolved("default");

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::Active { ref flags, .. }
            if !flags.contains(&ConversationActiveFlag::WaitingOnUserInput)
    ));
}

#[test]
fn terminal_turn_clears_active_flags_and_pending_requests() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.turn_started("default", "turn-1".to_string());
    manager.request_pending("default", pending_request(RequestId::Integer(7)));
    manager.interrupt_requested("default");

    manager.turn_finished("default", TurnViewStatus::Interrupted);

    let snapshot = manager.snapshot("default");
    assert!(matches!(snapshot.status, ConversationViewStatus::Idle));
    assert!(snapshot.pending_requests.is_empty());
}

#[test]
fn system_error_projects_terminal_error_status() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.turn_starting("default");
    manager.system_error("default", "failed before start".to_string());

    assert!(matches!(
        manager.snapshot("default").status,
        ConversationViewStatus::SystemError { ref message }
            if message == "failed before start"
    ));
}

#[test]
fn apply_runtime_updates_drive_snapshot_lifecycle() {
    let mut manager = ConversationRuntimeViewManager::default();

    let starting = manager.apply(ConversationRuntimeUpdate::TurnStarting {
        conversation_id: "default".to_string(),
    });
    assert!(matches!(
        starting.status,
        ConversationViewStatus::Active {
            active_turn_id: None,
            ref flags,
        } if flags.contains(&ConversationActiveFlag::RunningTurn)
    ));

    let started = manager.apply(ConversationRuntimeUpdate::TurnStarted {
        conversation_id: "default".to_string(),
        turn_id: "turn-1".to_string(),
    });
    assert!(matches!(
        started.status,
        ConversationViewStatus::Active {
            active_turn_id: Some(ref turn_id),
            ..
        } if turn_id == "turn-1"
    ));

    let pending = manager.apply(ConversationRuntimeUpdate::RequestPending {
        conversation_id: "default".to_string(),
        request: pending_request(RequestId::Integer(7)),
    });
    assert_eq!(pending.pending_requests.len(), 1);
    assert!(matches!(
        pending.status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::WaitingOnApproval)
    ));

    let resolved = manager.apply(ConversationRuntimeUpdate::RequestResolved {
        conversation_id: "default".to_string(),
        request_id: RequestId::Integer(7),
    });
    assert!(resolved.pending_requests.is_empty());
    assert!(matches!(
        resolved.status,
        ConversationViewStatus::Active { ref flags, .. }
            if !flags.contains(&ConversationActiveFlag::WaitingOnApproval)
    ));

    let finished = manager.apply(ConversationRuntimeUpdate::TurnFinished {
        conversation_id: "default".to_string(),
        final_status: TurnViewStatus::Completed,
    });
    assert!(matches!(finished.status, ConversationViewStatus::Idle));
}

#[test]
fn watch_subscriber_receives_latest_snapshot_and_updates() {
    let mut manager = ConversationRuntimeViewManager::default();

    manager.apply(ConversationRuntimeUpdate::TurnStarting {
        conversation_id: "default".to_string(),
    });
    let mut watcher = manager.subscribe("default");

    assert!(matches!(
        watcher.borrow().status,
        ConversationViewStatus::Active { ref flags, .. }
            if flags.contains(&ConversationActiveFlag::RunningTurn)
    ));

    manager.apply(ConversationRuntimeUpdate::TurnStarted {
        conversation_id: "default".to_string(),
        turn_id: "turn-1".to_string(),
    });

    watcher
        .has_changed()
        .expect("watch channel should remain open");
    assert!(matches!(
        watcher.borrow_and_update().status,
        ConversationViewStatus::Active {
            active_turn_id: Some(ref turn_id),
            ..
        } if turn_id == "turn-1"
    ));
}

fn pending_request(request_id: RequestId) -> PendingServerRequestView {
    PendingServerRequestView {
        request_id,
        conversation_id: "default".to_string(),
        turn_id: "turn-1".to_string(),
        kind: ServerRequestViewKind::CommandApproval,
        tool_name: "exec_command".to_string(),
        reason: "needs approval".to_string(),
        preview: "git status".to_string(),
        created_at_ms: 42,
    }
}
