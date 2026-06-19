use super::*;
use agent_core::{
    CommandExecutionStatus, CompactionContinuation, InputItem, TranscriptItem, TurnState,
};

#[test]
fn conversation_history_action_preserves_turns() {
    let message = AppServerMessage::Notification(AppServerNotification::ConversationHistory {
        conversation_id: "default".to_string(),
        turns: vec![ConversationTurn {
            id: "turn-1".to_string(),
            state: TurnState::Completed,
            items: vec![
                TranscriptItem::UserMessage {
                    id: "user:1".to_string(),
                    content: vec![InputItem::Text {
                        text: "hi".to_string(),
                    }],
                },
                TranscriptItem::CommandExecution {
                    id: "cmd:1".to_string(),
                    tool_name: "exec_command".to_string(),
                    command: "pwd".to_string(),
                    current_directory: "D:\\work".to_string(),
                    status: CommandExecutionStatus::Completed,
                    exit_code: Some(0),
                    output: Some("D:\\work".to_string()),
                    duration_ms: Some(1),
                    summary: "D:\\work".to_string(),
                },
                TranscriptItem::AgentMessage {
                    id: "assistant:1".to_string(),
                    text: "hello".to_string(),
                },
            ],
            rollout_start_index: 0,
            rollout_end_index: 1,
        }],
    });

    let reduced = apply_server_message(&message);

    assert!(reduced.actions.iter().any(|action| {
        matches!(
            action,
            ServerAction::ReplaceHistory(turns)
                if turns.len() == 1 && turns[0].id == "turn-1"
        )
    }));
}

#[test]
fn conversation_history_page_action_preserves_paging_metadata() {
    let message = AppServerMessage::Notification(AppServerNotification::ConversationHistoryPage {
        conversation_id: "default".to_string(),
        turns: vec![ConversationTurn {
            id: "turn-2".to_string(),
            state: TurnState::Completed,
            items: Vec::new(),
            rollout_start_index: 0,
            rollout_end_index: 1,
        }],
        has_more: true,
        next_before_turn_id: Some("turn-2".to_string()),
    });

    let reduced = apply_server_message(&message);

    assert!(reduced.actions.iter().any(|action| {
        matches!(
            action,
            ServerAction::ReplaceHistoryPage {
                turns,
                has_more: true,
                next_before_turn_id: Some(before),
            } if turns.len() == 1 && turns[0].id == "turn-2" && before == "turn-2"
        )
    }));
}

#[test]
fn token_usage_notification_updates_run_state() {
    let message = AppServerMessage::Notification(AppServerNotification::TokenUsageUpdated {
        conversation_id: "default".to_string(),
        turn_id: "turn-1".to_string(),
        last_usage: ModelUsage {
            input_tokens: 10,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_output_tokens: 1,
            total_tokens: 13,
        },
        total_usage: ModelUsage {
            input_tokens: 20,
            cached_input_tokens: 4,
            output_tokens: 6,
            reasoning_output_tokens: 2,
            total_tokens: 26,
        },
        model_context_window: Some(100),
    });

    let reduced = apply_server_message(&message);

    assert!(reduced.actions.iter().any(|action| {
        matches!(
            action,
            ServerAction::SetTokenUsage {
                last_usage,
                total_usage,
                model_context_window,
            } if last_usage.total_output_tokens() == 4
                && last_usage.total_tokens == 13
                && total_usage.cached_input_tokens == 4
                && *model_context_window == Some(100)
        )
    }));
}

#[test]
fn model_retrying_notification_sets_retry_status() {
    let message = AppServerMessage::Notification(AppServerNotification::ModelRetrying {
        conversation_id: "default".to_string(),
        turn_id: "turn-1".to_string(),
        stage: ModelRetryStage::Streaming,
        attempt: 2,
        next_delay_ms: 500,
    });

    let reduced = apply_server_message(&message);

    assert!(reduced.actions.iter().any(|action| {
        matches!(
            action,
            ServerAction::SetRetryStatus {
                stage,
                attempt,
                next_delay_ms,
            } if *stage == ModelRetryStage::Streaming
                && *attempt == 2
                && *next_delay_ms == 500
        )
    }));
}

#[test]
fn command_output_delta_updates_runtime_status() {
    let message =
        AppServerMessage::Notification(AppServerNotification::CommandExecutionOutputDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "cmd-1".to_string(),
            call_id: Some("call-1".to_string()),
            delta: "stdout".to_string(),
        });

    let reduced = apply_server_message(&message);

    assert!(reduced.actions.iter().any(|action| {
        matches!(
            action,
            ServerAction::AppendCommandOutputDelta { item_id, delta }
                if item_id == "cmd-1" && delta == "stdout"
        )
    }));
}

#[test]
fn generic_tool_output_deltas_wait_for_completed_item() {
    let tool_message = AppServerMessage::Notification(AppServerNotification::ToolOutputDelta {
        conversation_id: "default".to_string(),
        turn_id: "turn-1".to_string(),
        item_id: "tool-1".to_string(),
        call_id: Some("call-1".to_string()),
        delta: "large streaming tool output".to_string(),
    });
    let file_message =
        AppServerMessage::Notification(AppServerNotification::FileChangeOutputDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "edit-1".to_string(),
            call_id: Some("call-2".to_string()),
            delta: "patch output".to_string(),
        });

    let tool_reduced = apply_server_message(&tool_message);
    let file_reduced = apply_server_message(&file_message);

    assert!(
        tool_reduced
            .actions
            .iter()
            .all(|action| !matches!(action, ServerAction::AppendCommandOutputDelta { .. }))
    );
    assert!(
        file_reduced
            .actions
            .iter()
            .all(|action| !matches!(action, ServerAction::AppendCommandOutputDelta { .. }))
    );
}

#[test]
fn context_compaction_started_sets_runtime_status_without_notice_cell() {
    let message = AppServerMessage::Notification(AppServerNotification::ContextCompactionStarted {
        conversation_id: "default".to_string(),
        turn_id: Some("turn-1".to_string()),
        continuation: CompactionContinuation::MidTurn,
        estimated_tokens: 12_345,
    });

    let reduced = apply_server_message(&message);

    assert!(reduced.actions.iter().any(|action| {
        matches!(
            action,
            ServerAction::SetContextCompactionStatus { estimated_tokens }
                if *estimated_tokens == 12_345
        )
    }));
    assert!(
        !reduced
            .actions
            .iter()
            .any(|action| matches!(action, ServerAction::PushNoticeCell { .. }))
    );
}

#[test]
fn conversation_switched_only_updates_active_conversation() {
    let message = AppServerMessage::Notification(AppServerNotification::ConversationSwitched {
        conversation_id: "draft-1".to_string(),
    });

    let reduced = apply_server_message(&message);

    assert_eq!(reduced.actions.len(), 1);
    assert!(matches!(
        reduced.actions.first(),
        Some(ServerAction::SwitchConversation(conversation_id))
            if conversation_id == "draft-1"
    ));
}

#[test]
fn skills_changed_invalidates_local_skill_catalog() {
    let message = AppServerMessage::Notification(AppServerNotification::SkillsChanged {
        conversation_id: "default".to_string(),
    });

    let reduced = apply_server_message(&message);

    assert_eq!(reduced.actions.len(), 1);
    assert!(matches!(
        reduced.actions.first(),
        Some(ServerAction::InvalidateSkillsCatalog)
    ));
}

#[test]
fn transport_closed_error_finishes_active_turn() {
    let message = AppServerMessage::Notification(AppServerNotification::Error {
        conversation_id: "default".to_string(),
        message: "ERR_TRANSPORT_CLOSED: worker app server closed unexpectedly".to_string(),
    });

    let reduced = apply_server_message(&message);

    assert!(reduced.actions.iter().any(|action| matches!(
        action,
        ServerAction::TurnDispatch(TurnDispatch::Failed { error })
            if error == "worker app server closed unexpectedly"
    )));
    assert!(
        !reduced
            .actions
            .iter()
            .any(|action| matches!(action, ServerAction::PushErrorCell(_)))
    );
}
