use super::*;
use crate::app::conversation::facade::rebuild_transcript_from_history;
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
    assert_eq!(cells[0].body(), "complete answer");
    assert!(cells[0].label().is_empty());
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
        HistoryCell::info(
            "pwd",
            "current directory is D:\\learn\\gifti\\cloudagent",
            HistoryTone::Control,
        ),
    );

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(
        cells[0].body(),
        "current directory is D:\\learn\\gifti\\cloudagent"
    );
    assert_eq!(
        cells[0].tone,
        crate::ui::widgets::history_cell::HistoryTone::Control
    );
}

#[test]
fn control_start_shows_active_command_placeholder() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_control_item_started(
        "tool:1",
        TurnItemKind::CommandExecution,
        "cargo test --workspace",
    );

    let active = app
        .transcript_state
        .active_cell
        .as_ref()
        .expect("control start should create active placeholder");
    assert_eq!(active.label(), "Run command");
    assert_eq!(active.body(), "cargo test --workspace");
}

#[test]
fn control_start_replaces_prior_control_placeholder_without_flushing_running_history() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_control_item_started("tool:1", TurnItemKind::CommandExecution, "rg reasoning");
    app.handle_control_item_started("tool:2", TurnItemKind::CommandExecution, "cargo test");

    assert!(app.transcript_state.transcript.cells().is_empty());
    let active = app
        .transcript_state
        .active_cell
        .as_ref()
        .expect("second control should stay active");
    assert_eq!(active.body(), "cargo test");
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
fn ctrl_t_toggles_tool_detail_expansion_without_emitting_command() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );
    app.push_cell(HistoryCell::info(
        "pwd",
        "line 1\nline 2\nline 3",
        HistoryTone::Control,
    ));

    let result = app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert!(app.run_state.expand_tool_details);
    assert!(
        app.transcript_state
            .transcript
            .cells()
            .first()
            .is_some_and(|cell| cell.expanded)
    );
}

#[test]
fn ctrl_shift_t_also_toggles_tool_detail_expansion() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );
    app.push_cell(HistoryCell::info(
        "pwd",
        "line 1\nline 2\nline 3",
        HistoryTone::Control,
    ));

    let result = app.handle_key(KeyEvent::new(
        KeyCode::Char('T'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));

    assert!(result.is_none());
    assert!(app.run_state.expand_tool_details);
}

#[test]
fn slash_exit_parses_while_running() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );
    app.set_mode(agent_protocol::FrontendMode::Running);

    for ch in "/exit".chars() {
        let input = app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        assert!(input.is_none(), "typing should not submit before enter");
    }

    let input = app
        .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("enter should submit slash exit");

    assert!(matches!(
        input,
        ParsedInput::Command(AppClientCommand::Exit)
    ));
}

#[test]
fn slash_exit_parses_after_turn_cancelled() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );
    app.set_mode(agent_protocol::FrontendMode::Running);
    app.apply_turn_dispatch(TurnDispatch::Cancelled {
        reason: "interrupted".to_string(),
    });
    app.set_mode(agent_protocol::FrontendMode::Idle);

    for ch in "/exit".chars() {
        let input = app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        assert!(input.is_none(), "typing should not submit before enter");
    }

    let input = app
        .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .expect("enter should submit slash exit");

    assert!(matches!(
        input,
        ParsedInput::Command(AppClientCommand::Exit)
    ));
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
        HistoryCell::info("pwd", "D:\\learn\\gifti\\cloudagent", HistoryTone::Control),
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
fn reasoning_delta_streams_as_continuous_text() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_reasoning_item_started("reasoning:1", "reasoning");
    app.handle_reasoning_item_delta("reasoning:1", "用户");
    app.handle_reasoning_item_delta("reasoning:1", "在问");

    assert!(app.transcript_state.active_cell.is_none());
    assert_eq!(app.transcript_state.active_reasoning_text, "用户在问");
}

#[test]
fn reasoning_is_flushed_only_on_completion() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_reasoning_item_started("reasoning:1", "reasoning");
    app.handle_reasoning_item_delta("reasoning:1", "用户在问");
    assert!(app.transcript_state.transcript.cells().is_empty());

    app.handle_reasoning_item_completed("reasoning:1", "reasoning", "用户在问");

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].body(), "用户在问");
}

#[test]
fn reasoning_is_flushed_before_assistant_starts() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.handle_reasoning_item_started("reasoning:1", "reasoning");
    app.handle_reasoning_item_delta("reasoning:1", "先分析");
    app.handle_assistant_item_started("turn-1", "assistant:1");

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].body(), "先分析");
}

#[test]
fn history_rebuild_skips_reasoning_items() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    app.run_state.history_snapshot = Some(vec![agent_protocol::ConversationTurn {
        id: "turn-1".to_string(),
        state: agent_protocol::TurnState::Completed,
        items: vec![
            TranscriptItem::Reasoning {
                id: "reasoning:1".to_string(),
                title: "reasoning".to_string(),
                text: "先分析".to_string(),
            },
            TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "最终答复".to_string(),
            },
        ],
        rollout_start_index: 0,
        rollout_end_index: 1,
    }]);

    rebuild_transcript_from_history(&mut app);

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].body(), "最终答复");
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

    let first = HistoryCell::info("context", "workspace ready", HistoryTone::Control);
    let second = HistoryCell::info("context", "workspace ready", HistoryTone::Control);
    let third = HistoryCell::info("context", "workspace ready", HistoryTone::Control);
    app.push_cell(first);
    app.push_cell(second);
    app.push_cell(third);

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].repeat_count, 3);
    assert_eq!(cells[0].body(), "workspace ready");

    let pending: Vec<_> = app.pending_history_cells.iter().collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].repeat_count, 3);
    assert_eq!(pending[0].body(), "workspace ready");
}

#[test]
fn live_exploration_cells_stay_visible_until_history_rebuild() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    let mut first_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "text search `think`".to_string(),
    );
    first_aggregate.searches = 1;
    let first = HistoryCell::exploration(
        "Explored workspace",
        "searched 1 time",
        first_aggregate,
        HistoryTone::Control,
    );
    let mut second_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "cli/src/app/conversation/items.rs:1-200".to_string(),
    );
    second_aggregate.read_files = 1;
    let second = HistoryCell::exploration(
        "Explored workspace",
        "read 1 file",
        second_aggregate,
        HistoryTone::Control,
    );

    app.push_cell(first.clone());
    app.push_cell(second.clone());

    let live_cells = app.transcript_state.transcript.cells();
    assert_eq!(live_cells.len(), 2);
    assert_eq!(live_cells[0].body(), "searched 1 time");
    assert_eq!(live_cells[1].body(), "read 1 file");

    app.replace_history_cells(vec![first, second]);

    let rebuilt_cells = app.transcript_state.transcript.cells();
    assert_eq!(rebuilt_cells.len(), 1);
    assert!(rebuilt_cells[0].body().contains("searched 1 time"));
    assert!(rebuilt_cells[0].body().contains("read 1 file"));
}

#[test]
fn assistant_start_consolidates_prior_exploration_stage() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    let mut first_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "text search `think`".to_string(),
    );
    first_aggregate.searches = 1;
    let mut second_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "cli/src/app/conversation/items.rs:1-200".to_string(),
    );
    second_aggregate.read_files = 1;

    app.push_cell(HistoryCell::exploration(
        "Explored workspace",
        "searched 1 time",
        first_aggregate,
        HistoryTone::Control,
    ));
    app.push_cell(HistoryCell::exploration(
        "Explored workspace",
        "read 1 file",
        second_aggregate,
        HistoryTone::Control,
    ));

    assert_eq!(app.transcript_state.transcript.cells().len(), 2);

    app.handle_assistant_item_started("turn-1", "assistant:1");

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert!(cells[0].body().contains("searched 1 time"));
    assert!(cells[0].body().contains("read 1 file"));
}

#[test]
fn non_exploration_control_completion_consolidates_prior_exploration_stage() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    let mut first_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "text search `think`".to_string(),
    );
    first_aggregate.searches = 1;
    let mut second_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "cli/src/app/conversation/items.rs:1-200".to_string(),
    );
    second_aggregate.read_files = 1;

    app.push_cell(HistoryCell::exploration(
        "Explored workspace",
        "searched 1 time",
        first_aggregate,
        HistoryTone::Control,
    ));
    app.push_cell(HistoryCell::exploration(
        "Explored workspace",
        "read 1 file",
        second_aggregate,
        HistoryTone::Control,
    ));

    app.handle_control_item_completed(
        "tool:cmd",
        HistoryCell::exec(
            "Run command",
            "cargo test --workspace",
            Some("completed (exit 0)".to_string()),
            HistoryTone::Control,
        ),
    );

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 2);
    assert!(cells[0].body().contains("searched 1 time"));
    assert!(cells[0].body().contains("read 1 file"));
    assert_eq!(cells[1].body(), "cargo test --workspace");
}

#[test]
fn turn_completion_consolidates_trailing_exploration_stage() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("."),
        PathBuf::from("."),
        false,
        "ReadOnly".to_string(),
    );

    let mut first_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "text search `think`".to_string(),
    );
    first_aggregate.searches = 1;
    let mut second_aggregate = crate::ui::widgets::history_cell::ExplorationAggregate::new(
        "cli/src/app/conversation/items.rs:1-200".to_string(),
    );
    second_aggregate.read_files = 1;

    app.push_cell(HistoryCell::exploration(
        "Explored workspace",
        "searched 1 time",
        first_aggregate,
        HistoryTone::Control,
    ));
    app.push_cell(HistoryCell::exploration(
        "Explored workspace",
        "read 1 file",
        second_aggregate,
        HistoryTone::Control,
    ));

    app.apply_turn_dispatch(TurnDispatch::Completed);

    let cells = app.transcript_state.transcript.cells();
    assert_eq!(cells.len(), 1);
    assert!(cells[0].body().contains("searched 1 time"));
    assert!(cells[0].body().contains("read 1 file"));
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
    let bodies: Vec<&str> = cells.iter().map(|cell| cell.body()).collect();
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
    assert_eq!(cells[0].body(), "hello");
}

#[test]
fn clear_last_tool_name_does_not_revive_live_status_after_idle_mode() {
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
        ServerAction::SetMode(agent_protocol::FrontendMode::Running),
    );
    execute_server_action(&mut app, ServerAction::ClearLastToolName);
    assert_eq!(
        app.runtime_projection.live_label.as_deref(),
        Some("assistant is responding")
    );

    execute_server_action(
        &mut app,
        ServerAction::SetMode(agent_protocol::FrontendMode::Idle),
    );
    execute_server_action(&mut app, ServerAction::ClearLastToolName);

    assert_eq!(
        app.runtime_projection.phase,
        Some(crate::state::runtime_projection::RuntimePhase::Idle)
    );
    assert!(app.runtime_projection.live_label.is_none());
}

#[test]
fn clear_last_tool_name_preserves_waiting_approval_status() {
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
        ServerAction::SetMode(agent_protocol::FrontendMode::WaitingForServerRequest),
    );
    execute_server_action(&mut app, ServerAction::ClearLastToolName);

    assert_eq!(
        app.runtime_projection.phase,
        Some(crate::state::runtime_projection::RuntimePhase::WaitingApproval)
    );
    assert_eq!(
        app.runtime_projection.live_label.as_deref(),
        Some("waiting for approval")
    );
}
