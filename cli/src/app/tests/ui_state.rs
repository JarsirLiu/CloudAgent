use super::*;
use crate::state::NoticeLevel;
use crate::state::reducer::apply_server_message;

#[test]
fn mode_changes_do_not_clear_active_approval_view() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );
    app.input_pane
        .set_server_request(crate::ui::widgets::input_pane::ServerRequestInlineState {
            request_id: agent_protocol::RequestId::String("req-1".to_string()),
            title: "Run command?".to_string(),
            detail: "exec_command".to_string(),
        });

    app.set_mode(agent_protocol::FrontendMode::Running);

    assert!(app.input_pane.requires_action());
    assert_eq!(
        app.input_pane.active_server_request_id(),
        Some(agent_protocol::RequestId::String("req-1".to_string()))
    );
}

#[test]
fn assistant_delta_requires_item_started_before_streaming() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_assistant_item_delta("assistant:1", "partial");
    assert!(app.transcript_state.active_cell.is_none());

    app.handle_assistant_item_started("turn-1", "assistant:1");
    app.handle_assistant_item_completed("assistant:1", "complete answer");

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].body, "complete answer");
}

#[test]
fn tool_delta_requires_item_started_before_streaming() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_control_item_delta("tool:1", "half");
    assert!(app.transcript_state.active_cell.is_none());

    app.handle_control_item_started("tool:1", TurnItemKind::CommandExecution, "pwd");
    app.handle_control_item_completed(
        "tool:1",
        HistoryCell::from_message(
            "pwd",
            "current directory is D:\\learn\\gifti\\cloudagent",
            HistoryTone::Control,
        ),
    );

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(
        cells[0].body,
        "current directory is D:\\learn\\gifti\\cloudagent"
    );
    assert_eq!(
        cells[0].tone,
        crate::ui::widgets::history_cell::HistoryTone::Control
    );
}

#[test]
fn ctrl_d_exits_when_idle_if_composer_empty() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    let input = app
        .handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL))
        .expect("ctrl+d should produce exit input");

    assert!(matches!(
        input,
        ParsedInput::Command(AppClientCommand::Exit)
    ));
    assert!(app.run_state.should_exit);
}

#[test]
fn reasoning_and_control_cells_use_distinct_tones() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_reasoning_item_started("reasoning:1", "reasoning");
    app.handle_reasoning_item_delta("reasoning:1", "thinking");
    app.handle_reasoning_item_completed("reasoning:1", "reasoning", "thinking complete");
    app.handle_control_item_started("tool:1", TurnItemKind::CommandExecution, "pwd");
    app.handle_control_item_delta("tool:1", "pwd");
    app.handle_control_item_completed(
        "tool:1",
        HistoryCell::from_message("pwd", "D:\\learn\\gifti\\cloudagent", HistoryTone::Control),
    );

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 2);
    assert_eq!(
        cells[0].tone,
        crate::ui::widgets::history_cell::HistoryTone::Reasoning
    );
    assert_eq!(
        cells[1].tone,
        crate::ui::widgets::history_cell::HistoryTone::Control
    );
}

#[test]
fn repeated_control_cells_coalesce_and_pending_queue_stays_consistent() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    let first = HistoryCell::from_message("context", "workspace ready", HistoryTone::Control);
    let second = HistoryCell::from_message("context", "workspace ready", HistoryTone::Control);
    let third = HistoryCell::from_message("context", "workspace ready", HistoryTone::Control);
    app.push_cell(first);
    app.push_cell(second);
    app.push_cell(third);

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].repeat_count, 3);
    assert_eq!(cells[0].body, "workspace ready");

    let pending: Vec<_> = app.pending_history_cells.iter().collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].repeat_count, 3);
    assert_eq!(pending[0].body, "workspace ready");
}

#[test]
fn snapshot_history_replaces_transcript_without_event_replay() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    execute_server_action(
        &mut app,
        ServerAction::ReplaceHistory(vec![
            ConversationTurn {
                id: "turn-old".to_string(),
                state: agent_protocol::TurnState::Completed,
                rollout_start_index: 0,
                rollout_end_index: 1,
                items: vec![
                    TranscriptItem::UserMessage {
                        id: "user:old".to_string(),
                        text: "old question".to_string(),
                    },
                    TranscriptItem::AgentMessage {
                        id: "assistant:old".to_string(),
                        text: "old answer".to_string(),
                    },
                ],
            },
            ConversationTurn {
                id: "turn-where".to_string(),
                state: agent_protocol::TurnState::Completed,
                rollout_start_index: 2,
                rollout_end_index: 4,
                items: vec![
                    TranscriptItem::UserMessage {
                        id: "user:where".to_string(),
                        text: "where am i".to_string(),
                    },
                    TranscriptItem::ToolResult {
                        id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        content: "D:\\learn\\gifti\\cloudagent".to_string(),
                        summary: "D:\\learn\\gifti\\cloudagent".to_string(),
                        structured: Some(StructuredToolResult::CommandExecution {
                            command: "pwd".to_string(),
                            current_directory: "D:\\learn\\gifti\\cloudagent".to_string(),
                            session_id: None,
                            status: CommandExecutionStatus::Completed,
                            exit_code: Some(0),
                            success: Some(true),
                            stdout: Some("D:\\learn\\gifti\\cloudagent".to_string()),
                            stderr: Some(String::new()),
                            aggregated_output: Some("D:\\learn\\gifti\\cloudagent".to_string()),
                            duration_ms: Some(1),
                        }),
                    },
                    TranscriptItem::AgentMessage {
                        id: "assistant:cwd".to_string(),
                        text: "current directory is D:\\learn\\gifti\\cloudagent".to_string(),
                    },
                ],
            },
        ]),
    );

    let cells = app.transcript_state.transcript.cells();
    let bodies: Vec<&str> = cells.iter().map(|cell| cell.body.as_str()).collect();
    assert!(bodies.contains(&"old question"));
    assert!(bodies.contains(&"old answer"));
    assert!(bodies.contains(&"where am i"));
    assert!(bodies.contains(&"current directory is D:\\learn\\gifti\\cloudagent"));
}

#[test]
fn compaction_notifications_do_not_append_history_cells() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    execute_server_action(
        &mut app,
        ServerAction::SetSystemNotice {
            text: "existing".to_string(),
            level: NoticeLevel::Info,
        },
    );

    let reduce = apply_server_message(&AppServerMessage::Notification(
        AppServerNotification::ContextCompactionStarted {
            conversation_id: "default".to_string(),
            turn_id: "manual_compaction".to_string(),
            estimated_tokens: 123,
        },
    ));
    for action in reduce.actions {
        execute_server_action(&mut app, action);
    }

    let reduce = apply_server_message(&AppServerMessage::Notification(
        AppServerNotification::ContextCompacted {
            conversation_id: "default".to_string(),
            turn_id: "manual_compaction".to_string(),
            pre_context_tokens_estimate: 123,
            post_context_tokens_estimate: 45,
            pre_message_count: 10,
            post_message_count: 4,
            preserved_tail_count: 2,
        },
    ));
    for action in reduce.actions {
        execute_server_action(&mut app, action);
    }

    assert!(app.transcript_state.transcript.cells().is_empty());
    assert_eq!(
        app.run_state
            .system_notice
            .as_ref()
            .map(|notice| notice.text.as_str()),
        Some("Context compacted: ~123 -> ~45 tokens")
    );
}

#[test]
fn turn_dispatch_completed_flushes_active_assistant_cell() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );
    app.handle_assistant_item_started("turn-1", "assistant:flush");
    app.handle_assistant_item_delta("assistant:flush", "hello");
    app.apply_turn_dispatch(TurnDispatch::Completed);

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].body, "hello");
}
