use super::ConversationNotificationProjector;
use agent_core::{
    CommandExecutionStatus, CompactionContinuation, EventMsg, ModelRetryStage, ModelUsage,
    TranscriptItem, TurnItemDeltaKind, TurnItemKind, TurnState,
};
use agent_protocol::AppServerNotification;

#[test]
fn terminal_notifications_are_deferred_until_finish() {
    let mut projector = ConversationNotificationProjector::new("default");

    let immediate = projector.project_turn_event(&EventMsg::TurnCompleted {
        turn_id: "turn-1".to_string(),
    });
    assert!(immediate.is_empty());

    let flushed = projector.finish_turn(TurnState::Completed);
    assert_eq!(flushed.len(), 1);
    assert!(matches!(
        &flushed[0],
        AppServerNotification::TurnCompleted { turn_id, .. } if turn_id == "turn-1"
    ));
}

#[test]
fn system_error_projects_error_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    let notifications = projector.project_system_error("failed before start".to_string());

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::Error {
            conversation_id,
            message,
        } if conversation_id == "default" && message == "failed before start"
    ));
}

#[test]
fn file_change_output_delta_projects_to_file_change_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:write".to_string(),
        call_id: Some("call-write".to_string()),
        kind: TurnItemKind::FileChange,
        title: Some("edit_file".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "tool:write".to_string(),
        call_id: Some("call-write".to_string()),
        kind: TurnItemDeltaKind::FileChangeOutput,
        segment_index: None,
        delta: "wrote note.txt".to_string(),
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::FileChangeOutputDelta {
            item_id,
            call_id,
            delta,
            ..
        }
            if item_id == "tool:write"
                && call_id.as_deref() == Some("call-write")
                && delta == "wrote note.txt"
    ));
}

#[test]
fn command_execution_output_delta_projects_to_command_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        kind: TurnItemKind::CommandExecution,
        title: Some("exec_command".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        kind: TurnItemDeltaKind::CommandExecutionOutput,
        segment_index: None,
        delta: "D:\\work".to_string(),
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::CommandExecutionOutputDelta {
            item_id,
            call_id,
            delta,
            ..
        }
            if item_id == "tool:shell"
                && call_id.as_deref() == Some("call-shell")
                && delta == "D:\\work"
    ));
}

#[test]
fn generic_tool_output_delta_projects_to_tool_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:custom".to_string(),
        call_id: Some("call-custom".to_string()),
        kind: TurnItemKind::ToolCall,
        title: Some("custom_tool".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "tool:custom".to_string(),
        call_id: Some("call-custom".to_string()),
        kind: TurnItemDeltaKind::ToolOutput,
        segment_index: None,
        delta: "custom output".to_string(),
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::ToolOutputDelta {
            item_id,
            call_id,
            delta,
            ..
        }
            if item_id == "tool:custom"
                && call_id.as_deref() == Some("call-custom")
                && delta == "custom output"
    ));
}

#[test]
fn token_usage_projects_to_conversation_notification() {
    let mut projector = ConversationNotificationProjector::new("default");
    let usage = ModelUsage {
        input_tokens: 100,
        cached_input_tokens: 25,
        output_tokens: 40,
        reasoning_output_tokens: 5,
        total_tokens: 140,
    };

    let notifications = projector.project_turn_event(&EventMsg::TokenUsageUpdated {
        turn_id: "turn-1".to_string(),
        last_usage: usage.clone(),
        total_usage: usage.clone(),
        model_context_window: Some(1000),
        request_estimated_tokens: 130,
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::TokenUsageUpdated {
            conversation_id,
            turn_id,
            last_usage,
            total_usage,
            model_context_window,
        } if conversation_id == "default"
            && turn_id == "turn-1"
            && last_usage.total_tokens == 140
            && total_usage.cached_input_tokens == 25
            && *model_context_window == Some(1000)
    ));
}

#[test]
fn model_retrying_projects_to_conversation_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    let notifications = projector.project_turn_event(&EventMsg::ModelRetrying {
        turn_id: "turn-1".to_string(),
        stage: ModelRetryStage::Streaming,
        attempt: 2,
        next_delay_ms: 500,
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::ModelRetrying {
            conversation_id,
            turn_id,
            stage,
            attempt,
            next_delay_ms,
        } if conversation_id == "default"
            && turn_id == "turn-1"
            && *stage == ModelRetryStage::Streaming
            && *attempt == 2
            && *next_delay_ms == 500
    ));
}

#[test]
fn context_compaction_notifications_preserve_continuation() {
    let mut projector = ConversationNotificationProjector::new("default");

    let started = projector.project_turn_event(&EventMsg::ContextCompactionStarted {
        turn_id: "turn-1".to_string(),
        continuation: CompactionContinuation::MidTurn,
        estimated_tokens: 12_345,
    });
    let compacted = projector.project_turn_event(&EventMsg::ContextCompacted {
        turn_id: "turn-1".to_string(),
        continuation: CompactionContinuation::MidTurn,
        pre_context_tokens_estimate: 12_345,
        post_context_tokens_estimate: 4_321,
        pre_message_count: 20,
        post_message_count: 6,
        preserved_user_count: 4,
    });

    assert!(matches!(
        &started[0],
        AppServerNotification::ContextCompactionStarted {
            continuation,
            estimated_tokens,
            ..
        } if *continuation == CompactionContinuation::MidTurn
            && *estimated_tokens == 12_345
    ));
    assert!(matches!(
        &compacted[0],
        AppServerNotification::ContextCompacted {
            continuation,
            pre_context_tokens_estimate,
            post_context_tokens_estimate,
            ..
        } if *continuation == CompactionContinuation::MidTurn
            && *pre_context_tokens_estimate == 12_345
            && *post_context_tokens_estimate == 4_321
    ));
}

#[test]
fn assistant_text_delta_projects_to_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemKind::AssistantMessage,
        title: Some("assistant_message".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemDeltaKind::Text,
        segment_index: None,
        delta: "hello".to_string(),
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::AgentMessageDelta {
            conversation_id,
            turn_id,
            item_id,
            delta,
        } if conversation_id == "default"
            && turn_id == "turn-1"
            && item_id == "assistant:1"
            && delta == "hello"
    ));
}

#[test]
fn reasoning_text_delta_projects_to_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemKind::Reasoning,
        title: Some("reasoning".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemDeltaKind::ReasoningText,
        segment_index: None,
        delta: "step".to_string(),
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::ReasoningTextDelta {
            conversation_id,
            turn_id,
            item_id,
            delta,
            ..
        } if conversation_id == "default"
            && turn_id == "turn-1"
            && item_id == "reasoning:1"
            && delta == "step"
    ));
}

#[test]
fn reasoning_summary_delta_projects_to_notification() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemKind::Reasoning,
        title: Some("reasoning".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemDeltaKind::ReasoningSummary,
        segment_index: None,
        delta: "summary".to_string(),
    });

    assert_eq!(notifications.len(), 2);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::ReasoningSummaryPartAdded {
            conversation_id,
            turn_id,
            item_id,
            summary_index,
        } if conversation_id == "default"
            && turn_id == "turn-1"
            && item_id == "reasoning:1"
            && *summary_index == 0
    ));
    assert!(matches!(
        &notifications[1],
        AppServerNotification::ReasoningSummaryTextDelta {
            conversation_id,
            turn_id,
            item_id,
            summary_index,
            delta,
            ..
        } if conversation_id == "default"
            && turn_id == "turn-1"
            && item_id == "reasoning:1"
            && *summary_index == 0
            && delta == "summary"
    ));
}

#[test]
fn item_completed_clears_active_lifecycle() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemKind::AssistantMessage,
        title: Some("assistant_message".to_string()),
    });
    let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        item: TranscriptItem::AgentMessage {
            id: "assistant:1".to_string(),
            text: "done".to_string(),
        },
    });
    let terminal = projector.project_turn_event(&EventMsg::TurnCompleted {
        turn_id: "turn-1".to_string(),
    });
    let flushed = projector.finish_turn(TurnState::Completed);

    assert!(matches!(
        completed.as_slice(),
        [AppServerNotification::ItemCompleted { item, .. }]
            if matches!(
                item,
                TranscriptItem::AgentMessage { id, text }
                    if id == "assistant:1" && text == "done"
            )
    ));
    assert!(terminal.is_empty());
    assert_eq!(flushed.len(), 1);
    assert!(
        flushed.iter().any(|notification| matches!(
            notification,
            AppServerNotification::TurnCompleted { .. }
        ))
    );
    assert!(
        !flushed
            .iter()
            .any(|notification| matches!(notification, AppServerNotification::Error { .. }))
    );
}

#[test]
fn assistant_item_completed_projects_final_source() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemKind::AssistantMessage,
        title: Some("assistant_message".to_string()),
    });
    let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        item: TranscriptItem::AgentMessage {
            id: "assistant:1".to_string(),
            text: "done".to_string(),
        },
    });

    assert!(matches!(
        completed.as_slice(),
        [AppServerNotification::ItemCompleted {
            conversation_id,
            turn_id,
            call_id,
            item,
        }] if conversation_id == "default"
            && turn_id == "turn-1"
            && call_id.is_none()
            && matches!(
                item,
                TranscriptItem::AgentMessage { id, text }
                    if id == "assistant:1" && text == "done"
            )
    ));
}

#[test]
fn reasoning_item_completed_projects_final_source() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemKind::Reasoning,
        title: Some("thinking".to_string()),
    });
    let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        item: TranscriptItem::Reasoning {
            id: "reasoning:1".to_string(),
            title: "thinking".to_string(),
            text: "final reasoning".to_string(),
        },
    });

    assert!(matches!(
        completed.as_slice(),
        [AppServerNotification::ItemCompleted {
            conversation_id,
            turn_id,
            call_id,
            item,
        }] if conversation_id == "default"
            && turn_id == "turn-1"
            && call_id.is_none()
            && matches!(
                item,
                TranscriptItem::Reasoning { id, title, text }
                    if id == "reasoning:1"
                        && title == "thinking"
                        && text == "final reasoning"
            )
    ));
}

#[test]
fn mismatched_call_id_reports_lifecycle_error() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        kind: TurnItemKind::CommandExecution,
        title: Some("exec_command".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-other".to_string()),
        kind: TurnItemDeltaKind::CommandExecutionOutput,
        segment_index: None,
        delta: "D:\\work".to_string(),
    });

    assert_eq!(notifications.len(), 1);
    assert!(matches!(
        &notifications[0],
        AppServerNotification::Error { message, .. }
            if message.contains("call `call-other` received lifecycle event before item start")
    ));
}

#[test]
fn item_completed_prefers_event_call_id_when_lifecycle_is_missing() {
    let mut projector = ConversationNotificationProjector::new("default");

    let completed = projector.project_turn_event(&EventMsg::ItemCompleted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        item: TranscriptItem::ToolResult {
            id: "tool:shell".to_string(),
            tool_name: "exec_command".to_string(),
            content: "done".to_string(),
            summary: "done".to_string(),
            structured: None,
        },
    });

    assert!(matches!(
        completed.last(),
        Some(AppServerNotification::ItemCompleted { call_id, .. })
            if call_id.as_deref() == Some("call-shell")
    ));
}

#[test]
fn completed_item_removes_call_id_index() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        kind: TurnItemKind::CommandExecution,
        title: Some("exec_command".to_string()),
    });
    let _ = projector.project_turn_event(&EventMsg::ItemCompleted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        item: TranscriptItem::CommandExecution {
            id: "tool:shell".to_string(),
            tool_name: "exec_command".to_string(),
            command: "pwd".to_string(),
            current_directory: "D:\\work".to_string(),
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            output: Some("D:\\work".to_string()),
            duration_ms: Some(1),
            summary: "D:\\work".to_string(),
        },
    });

    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "tool:other".to_string(),
        call_id: Some("call-shell".to_string()),
        kind: TurnItemDeltaKind::CommandExecutionOutput,
        segment_index: None,
        delta: "should fail".to_string(),
    });

    assert!(matches!(
        notifications.first(),
        Some(AppServerNotification::Error { message, .. })
            if message.contains("call `call-shell` received lifecycle event before item start")
    ));
}

#[test]
fn call_id_turn_mismatch_reports_lifecycle_error() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        kind: TurnItemKind::CommandExecution,
        title: Some("exec_command".to_string()),
    });
    let notifications = projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-2".to_string(),
        item_id: "tool:shell".to_string(),
        call_id: Some("call-shell".to_string()),
        kind: TurnItemDeltaKind::CommandExecutionOutput,
        segment_index: None,
        delta: "D:\\other".to_string(),
    });

    assert!(matches!(
        notifications.first(),
        Some(AppServerNotification::Error { message, .. })
            if message.contains("call `call-shell` belongs to turn `turn-1`")
    ));
}

#[test]
fn turn_completed_reports_dangling_active_items() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemKind::AssistantMessage,
        title: Some("assistant_message".to_string()),
    });
    projector.project_turn_event(&EventMsg::TurnCompleted {
        turn_id: "turn-1".to_string(),
    });
    let flushed = projector.finish_turn(TurnState::Completed);

    assert!(flushed.iter().any(|notification| matches!(
        notification,
        AppServerNotification::Error { message, .. }
            if message.contains("completed with active items")
    )));
}

#[test]
fn stable_item_ids_preserve_arrival_order_when_reasoning_starts_late() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemKind::AssistantMessage,
        title: Some("assistant_message".to_string()),
    });
    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemKind::Reasoning,
        title: Some("reasoning".to_string()),
    });

    let ordered = projector.stable_item_ids_for_turn("turn-1");
    assert_eq!(ordered, vec!["assistant:1", "reasoning:1"]);
}

#[test]
fn tool_items_preserve_arrival_order_relative_to_assistant() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemKind::AssistantMessage,
        title: Some("assistant_message".to_string()),
    });
    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:1".to_string(),
        call_id: Some("call-1".to_string()),
        kind: TurnItemKind::CommandExecution,
        title: Some("exec_command".to_string()),
    });
    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemKind::Reasoning,
        title: Some("reasoning".to_string()),
    });

    let ordered = projector.stable_item_ids_for_turn("turn-1");
    assert_eq!(ordered, vec!["assistant:1", "tool:1", "reasoning:1"]);
}

#[test]
fn active_turn_snapshot_preserves_late_reasoning_after_assistant_when_it_arrives_late() {
    let mut projector = ConversationNotificationProjector::new("default");

    projector.project_turn_event(&EventMsg::TurnStarted {
        turn_id: "turn-1".to_string(),
        conversation_id: "default".to_string(),
        user_input: agent_core::text_input_items("hi"),
    });
    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemKind::Reasoning,
        title: Some("reasoning".to_string()),
    });
    projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        call_id: None,
        kind: TurnItemDeltaKind::ReasoningSummary,
        segment_index: None,
        delta: "first".to_string(),
    });
    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "tool:1".to_string(),
        call_id: Some("call-1".to_string()),
        kind: TurnItemKind::CommandExecution,
        title: Some("pwd".to_string()),
    });
    projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "tool:1".to_string(),
        call_id: Some("call-1".to_string()),
        kind: TurnItemDeltaKind::CommandExecutionOutput,
        segment_index: None,
        delta: "D:\\work".to_string(),
    });
    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemKind::AssistantMessage,
        title: Some("assistant_message".to_string()),
    });
    projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "assistant:1".to_string(),
        call_id: None,
        kind: TurnItemDeltaKind::Text,
        segment_index: None,
        delta: "answer".to_string(),
    });
    projector.project_turn_event(&EventMsg::ItemStarted {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:2".to_string(),
        call_id: None,
        kind: TurnItemKind::Reasoning,
        title: Some("reasoning".to_string()),
    });
    projector.project_turn_event(&EventMsg::ItemDelta {
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:2".to_string(),
        call_id: None,
        kind: TurnItemDeltaKind::ReasoningSummary,
        segment_index: None,
        delta: "second".to_string(),
    });

    let snapshot = projector
        .active_turn_snapshot()
        .expect("active turn snapshot should exist");

    let ids = snapshot
        .items
        .iter()
        .map(|item| item.id().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "user:turn-1",
            "reasoning:1",
            "tool:1",
            "assistant:1",
            "reasoning:2",
        ]
    );
}
