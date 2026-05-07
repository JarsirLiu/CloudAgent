use crate::app::core::transcript_owner::TranscriptOwner;
use crate::app::conversation::facade as conversation_facade;
use crate::app::conversation::actions::execute_server_action;
use crate::app::TuiApp;
use crate::app::runtime::display::should_show_welcome;
use crate::ui::chat_surface_model::{ChatSurfaceBody, build_chat_surface_model};
use crate::ui::chat_surface::ChatSurface;
use agent_protocol::CommandExecutionStatus;
use agent_protocol::{
    ConversationTurn, ReadFileEntry, ReadFileStatus, SearchWorkspaceMode,
    SearchWorkspaceOperation, SearchWorkspaceStatus, StructuredToolResult, TranscriptItem,
    TurnState,
};
use std::path::PathBuf;

fn user(id: &str, text: &str) -> TranscriptItem {
    TranscriptItem::UserMessage {
        id: id.to_string(),
        text: text.to_string(),
    }
}

fn reasoning(id: &str, text: &str) -> TranscriptItem {
    TranscriptItem::Reasoning {
        id: id.to_string(),
        title: "Reasoning".to_string(),
        text: text.to_string(),
    }
}

fn agent(id: &str, text: &str) -> TranscriptItem {
    TranscriptItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
    }
}

fn command(id: &str, command: &str) -> TranscriptItem {
    TranscriptItem::CommandExecution {
        id: id.to_string(),
        tool_name: "exec_command".to_string(),
        command: command.to_string(),
        current_directory: "D:\\learn\\gifti\\cloudagent".to_string(),
        status: CommandExecutionStatus::Completed,
        exit_code: Some(0),
        stdout: Some(String::new()),
        stderr: Some(String::new()),
        aggregated_output: Some(String::new()),
        duration_ms: Some(1),
        summary: command.to_string(),
    }
}

fn read_file_result(id: &str, path: &str) -> TranscriptItem {
    TranscriptItem::ToolResult {
        id: id.to_string(),
        tool_name: "read_file".to_string(),
        content: String::new(),
        summary: String::new(),
        structured: Some(StructuredToolResult::ReadFile {
            path: path.to_string(),
            start_line: Some(1),
            max_lines: Some(1),
            total_chars: 10,
            read: ReadFileEntry {
                path: path.to_string(),
                start_line: Some(1),
                end_line: Some(1),
                next_start_line: None,
                returned_line_count: 1,
                total_line_count: Some(1),
                returned_char_count: 10,
                truncated: false,
                char_count: 10,
                status: ReadFileStatus::Ok,
                version_token: None,
            },
        }),
    }
}

fn search_workspace_result(id: &str, query: &str) -> TranscriptItem {
    TranscriptItem::ToolResult {
        id: id.to_string(),
        tool_name: "search_workspace".to_string(),
        content: String::new(),
        summary: String::new(),
        structured: Some(StructuredToolResult::SearchWorkspace {
            session_id: "search:test:1".to_string(),
            operation: SearchWorkspaceOperation::Search,
            mode: SearchWorkspaceMode::Text,
            status: SearchWorkspaceStatus::Active,
            query: query.to_string(),
            path_scope: None,
            case_sensitive: false,
            context_lines: 0,
            max_results: 20,
            offset: 0,
            file_count: 2,
            match_count: 3,
            truncated: false,
            next_offset: None,
            hits: Vec::new(),
        }),
    }
}

fn turn(id: &str, state: TurnState, items: Vec<TranscriptItem>) -> ConversationTurn {
    ConversationTurn {
        id: id.to_string(),
        state,
        rollout_start_index: 0,
        rollout_end_index: items.len(),
        items,
    }
}

#[test]
fn transcript_owner_shows_local_user_immediately() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user("hello".to_string(), false);

    assert!(owner.live_cells().is_empty());
    let pending = owner
        .pending_history_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(pending, vec!["hello"]);
}

#[test]
fn local_live_cells_replace_previous_active_notice() {
    let mut owner = TranscriptOwner::default();
    owner.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::info(
        "conversation",
        "first notice",
        crate::ui::widgets::history_cell::HistoryTone::Warning,
    ));
    owner.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::info(
        "conversation",
        "second notice",
        crate::ui::widgets::history_cell::HistoryTone::Warning,
    ));

    let live = owner.live_cells();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].body(), "second notice");
    assert_eq!(owner.active_cell().map(|cell| cell.body()), Some("second notice"));
}

#[test]
fn transcript_owner_rebuilds_history_and_keeps_only_live_tail_active() {
    let mut owner = TranscriptOwner::default();
    let history = vec![
        turn("turn-1", TurnState::Completed, vec![user("u1", "first"), agent("a1", "done")]),
        turn(
            "turn-2",
            TurnState::Running,
            vec![user("u2", "second"), reasoning("r2", "thinking")],
        ),
    ];

    owner.rebuild_from_history_snapshot(&history, false);

    let pending = owner
        .pending_history_cells()
        .iter()
        .map(|cell| (cell.body().to_string(), cell.is_stream_continuation()))
        .collect::<Vec<_>>();

    assert_eq!(
        pending,
        vec![
            ("first".to_string(), false),
            ("done".to_string(), false),
            ("second".to_string(), false)
        ]
    );
    let visible = owner
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(visible, vec!["thinking"]);
}

#[test]
fn completed_turn_history_merges_only_adjacent_agent_message_runs() {
    let mut owner = TranscriptOwner::default();
    let history = vec![turn(
        "turn-1",
        TurnState::Completed,
        vec![
            user("u1", "first"),
            agent("a1", "part one"),
            agent("a2", "part two"),
            reasoning("r1", "thinking"),
            agent("a3", "done"),
        ],
    )];

    owner.rebuild_from_history_snapshot(&history, false);

    let cells = owner
        .pending_history_cells()
        .iter()
        .map(|cell| (cell.body().to_string(), cell.is_stream_continuation()))
        .collect::<Vec<_>>();

    assert_eq!(
        cells,
        vec![
            ("first".to_string(), false),
            ("part onepart two".to_string(), false),
            ("thinking".to_string(), false),
            ("done".to_string(), false),
        ]
    );
}

#[test]
fn transcript_owner_keeps_streaming_turn_visible_across_item_boundaries() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user("hello".to_string(), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        agent_protocol::TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );
    owner.append_reasoning_delta(
        "turn-1".to_string(),
        "r1".to_string(),
        "thinking".to_string(),
        false,
    );
    owner.complete_item(
        "turn-1".to_string(),
        "r1".to_string(),
        reasoning("r1", "thinking"),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "c1".to_string(),
        agent_protocol::TurnItemKind::CommandExecution,
        Some("Run command".to_string()),
        false,
    );
    owner.append_output_delta(
        "turn-1".to_string(),
        "c1".to_string(),
        "rg *".to_string(),
        false,
    );

    let visible = owner
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(visible.len(), 1);
    assert!(
        visible[0].contains("rg *") || visible[0].contains("inspect command"),
        "visible: {visible:?}"
    );

    owner.complete_item(
        "turn-1".to_string(),
        "c1".to_string(),
        command("c1", "rg *"),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent_protocol::TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "done".to_string(),
        false,
    );

    let visible = owner
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(visible, vec!["done"]);
}

#[test]
fn completing_older_item_does_not_flush_current_live_item() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user("hello".to_string(), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent_protocol::TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "partial answer".to_string(),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "c1".to_string(),
        agent_protocol::TurnItemKind::CommandExecution,
        Some("Run command".to_string()),
        false,
    );
    owner.append_output_delta(
        "turn-1".to_string(),
        "c1".to_string(),
        "rg esc".to_string(),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent("a1", "partial answer"),
        false,
    );

    let visible = owner
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(visible.len(), 1);
    assert!(
        visible[0].contains("rg esc") || visible[0].contains("inspect command"),
        "visible: {visible:?}"
    );
}

#[test]
fn runtime_non_agent_cells_do_not_become_stream_continuations() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user("hello".to_string(), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        agent_protocol::TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );
    owner.append_reasoning_delta(
        "turn-1".to_string(),
        "r1".to_string(),
        "thinking".to_string(),
        false,
    );
    owner.complete_item(
        "turn-1".to_string(),
        "r1".to_string(),
        reasoning("r1", "thinking"),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "c1".to_string(),
        agent_protocol::TurnItemKind::CommandExecution,
        Some("Run command".to_string()),
        false,
    );
    owner.append_output_delta(
        "turn-1".to_string(),
        "c1".to_string(),
        "rg cli".to_string(),
        false,
    );
    owner.complete_item(
        "turn-1".to_string(),
        "c1".to_string(),
        command("c1", "rg cli"),
        false,
    );

    let cells = owner
        .pending_history_cells()
        .iter()
        .map(|cell| (cell.body().to_string(), cell.is_stream_continuation()))
        .collect::<Vec<_>>();

    assert_eq!(
        cells,
        vec![
            ("hello".to_string(), false),
            ("thinking".to_string(), false),
            ("ran 1 inspect command".to_string(), false),
        ]
    );
}

#[test]
fn tool_result_completion_replaces_matching_toolcall_placeholder() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user("hello".to_string(), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "toolcall-1".to_string(),
        agent_protocol::TurnItemKind::ToolCall,
        Some("read_file".to_string()),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "toolresult-1".to_string(),
        read_file_result("toolresult-1", "D:\\learn\\gifti\\cloudagent\\README.md"),
        false,
    );

    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent_protocol::TurnItemKind::AssistantMessage,
        None,
        false,
    );

    let pending = owner
        .pending_history_cells()
        .iter()
        .map(|cell| format!("{}|{}", cell.label(), cell.body()))
        .collect::<Vec<_>>();

    assert_eq!(owner.live_cells().len(), 1);
    assert!(
        pending.iter().any(|entry| entry.contains("Explored workspace|read 1 file")),
        "pending: {pending:?}"
    );
    assert!(
        pending.iter().all(|entry| !entry.contains("Read file|running")),
        "pending: {pending:?}"
    );
}

#[test]
fn parallel_toolcall_placeholders_do_not_commit_running_cards() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user("hello".to_string(), false);
    owner.bind_turn_id("turn-1".to_string(), false);

    owner.start_item(
        "turn-1".to_string(),
        "toolcall-1".to_string(),
        agent_protocol::TurnItemKind::ToolCall,
        Some("read_file".to_string()),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "toolcall-2".to_string(),
        agent_protocol::TurnItemKind::ToolCall,
        Some("read_file".to_string()),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "toolcall-3".to_string(),
        agent_protocol::TurnItemKind::ToolCall,
        Some("read_file".to_string()),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent_protocol::TurnItemKind::AssistantMessage,
        None,
        false,
    );

    let pending = owner
        .pending_history_cells()
        .iter()
        .map(|cell| format!("{}|{}", cell.label(), cell.body()))
        .collect::<Vec<_>>();

    assert!(
        pending.iter().all(|entry| !entry.contains("Read file|running")),
        "pending: {pending:?}"
    );
}

#[test]
fn adjacent_exploration_history_cells_merge_without_crossing_reasoning_barrier() {
    let mut owner = TranscriptOwner::default();
    let history = vec![turn(
        "turn-1",
        TurnState::Completed,
        vec![
            user("u1", "hello"),
            read_file_result("rf1", "D:\\learn\\gifti\\cloudagent\\README.md"),
            search_workspace_result("sw1", "Interrupt"),
            reasoning("r1", "thinking"),
            read_file_result("rf2", "D:\\learn\\gifti\\cloudagent\\cli\\src\\main.rs"),
        ],
    )];

    owner.rebuild_from_history_snapshot(&history, false);

    let pending = owner
        .pending_history_cells()
        .iter()
        .map(|cell| (cell.label().to_string(), cell.body().to_string()))
        .collect::<Vec<_>>();

    assert_eq!(
        pending,
        vec![
            ("you".to_string(), "hello".to_string()),
            (
                "Explored workspace".to_string(),
                "searched 1 time, read 1 file".to_string(),
            ),
            ("Reasoning".to_string(), "thinking".to_string()),
            ("Explored workspace".to_string(), "read 1 file".to_string()),
        ]
    );
}

#[test]
fn running_snapshot_updates_history_cache_without_touching_live_transcript() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        agent_protocol::TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );
    app.transcript_owner.append_reasoning_delta(
        "turn-1".to_string(),
        "r1".to_string(),
        "thinking".to_string(),
        false,
    );

    let before = app
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    conversation_facade::upsert_turn_snapshot(
        &mut app,
        turn(
            "turn-1",
            TurnState::Running,
            vec![user("u1", "hello"), reasoning("r1", "thinking from snapshot")],
        ),
    );

    let after = app
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(before, after);
    let history = app.run_state.history_snapshot.clone().unwrap_or_default();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, "turn-1");
    assert_eq!(history[0].state, TurnState::Running);
}

#[test]
fn chat_surface_model_renders_streaming_visible_tail() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        agent_protocol::TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );
    app.transcript_owner.append_reasoning_delta(
        "turn-1".to_string(),
        "r1".to_string(),
        "thinking".to_string(),
        false,
    );

    let model = build_chat_surface_model(&mut app, 80, 12);
    let ChatSurfaceBody::ActiveCell(active) = model.body else {
        panic!("expected active cell body");
    };

    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert!(
        rendered.iter().any(|line| line.contains("thinking")),
        "rendered: {rendered:?}"
    );
}

#[test]
fn streaming_reasoning_stays_fully_visible_until_completion() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        agent_protocol::TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );
    app.transcript_owner.append_reasoning_delta(
        "turn-1".to_string(),
        "r1".to_string(),
        "First paragraph with enough text to wrap across multiple lines.\n\nSecond paragraph with another long explanation that should remain visible while reasoning is still active.".to_string(),
        false,
    );

    let model = build_chat_surface_model(&mut app, 80, 40);
    let ChatSurfaceBody::ActiveCell(active) = model.body else {
        panic!("expected active cell body");
    };

    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("First paragraph"));
    assert!(rendered.contains("Second paragraph"));
    assert!(!rendered.contains("Ctrl+T toggles details"));
}

#[test]
fn chat_surface_model_renders_placeholder_before_first_delta() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        agent_protocol::TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );

    let model = build_chat_surface_model(&mut app, 80, 12);
    let ChatSurfaceBody::ActiveCell(active) = model.body else {
        panic!("expected active cell body");
    };

    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    assert!(
        rendered.iter().any(|line| line.contains("thinking")),
        "rendered: {rendered:?}"
    );
}

#[test]
fn committed_history_without_active_cell_does_not_allocate_active_body_lines() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);

    let model = build_chat_surface_model(&mut app, 80, 20);
    let ChatSurfaceBody::ActiveCell(active) = model.body else {
        panic!("expected active cell body");
    };

    assert!(active.lines.is_empty(), "lines: {:?}", active.lines);
    assert_eq!(model.body_height, 0);
}

#[test]
fn committed_history_without_active_cell_keeps_viewport_compact() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);

    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let desired = ChatSurface::desired_viewport_height(&mut app, terminal_area);
    let bottom_only = app.bottom_pane.desired_height(app.current_mode(), 120).max(1);

    assert_eq!(desired, bottom_only.saturating_add(2));
}

#[test]
fn welcome_stays_visible_while_composer_has_draft_text() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    let _ = app.bottom_pane.handle_paste("draft message");

    assert!(should_show_welcome(&app));
    let model = build_chat_surface_model(&mut app, 80, 20);
    assert!(matches!(model.body, ChatSurfaceBody::Welcome));
}

#[test]
fn reset_local_view_requests_history_replay() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);
    app.reset_local_view();

    let plan = app
        .terminal_projection
        .build_plan(&mut app.transcript_owner, 5, 80);
    match plan.history_update {
        crate::terminal::HistoryUpdate::ReplayAll(cells) => assert!(cells.is_empty()),
        crate::terminal::HistoryUpdate::AppendTail(_) => panic!("expected replay-all after reset"),
    }
}

#[test]
fn reset_notice_is_suppressed_after_local_clear() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.reset_local_view();
    app.arm_reset_notice_suppression();
    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::PushNoticeCell {
            label: "conversation".to_string(),
            message: "conversation reset".to_string(),
            level: crate::state::NoticeLevel::Info,
        },
    );

    assert!(should_show_welcome(&app));
    assert!(app.transcript_owner.active_cell().is_none());
    assert!(!app.transcript_owner.has_transcript_content());
}

#[test]
fn generic_live_notice_does_not_keep_mode_running() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::info(
        "conversation",
        "no active turn",
        crate::ui::widgets::history_cell::HistoryTone::Warning,
    ));

    assert_eq!(app.current_mode(), agent_protocol::FrontendMode::Idle);
    assert!(app.can_submit_turn());
}

#[test]
fn cancelled_turn_clears_running_state_and_reenables_submit() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.prepare_submitted_turn("hello");
    assert_eq!(app.current_mode(), agent_protocol::FrontendMode::Running);
    assert!(!app.can_submit_turn());

    app.apply_turn_dispatch(crate::state::reducer::TurnDispatch::Cancelled {
        reason: "interrupted".to_string(),
    });

    assert_eq!(app.current_mode(), agent_protocol::FrontendMode::Idle);
    assert!(app.can_submit_turn());
    assert!(app.transcript_owner.active_turn_id().is_none());
    assert_eq!(
        app.transcript_owner.active_cell().map(|cell| cell.body()),
        Some("interrupted")
    );
}

#[test]
fn active_reasoning_transcript_matches_reasoning_card_ui() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::reasoning(
        "Reasoning",
        "Let me inspect the code path carefully.".to_string(),
    ));

    let model = build_chat_surface_model(&mut app, 80, 20);
    let ChatSurfaceBody::ActiveCell(active) = model.body else {
        panic!("expected active cell body");
    };
    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("≈ "));
    assert!(rendered.contains("Reasoning"));
    assert!(rendered.contains("│ "));
    assert!(rendered.contains("Let me inspect the code path carefully."));
}

#[test]
fn active_notice_transcript_does_not_render_history_rails() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::info(
        "conversation",
        "conversation reset",
        crate::ui::widgets::history_cell::HistoryTone::Control,
    ));

    let model = build_chat_surface_model(&mut app, 80, 20);
    let ChatSurfaceBody::ActiveCell(active) = model.body else {
        panic!("expected active cell body");
    };
    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("conversation"));
    assert!(!rendered.contains("│ "));
}

#[test]
fn finalized_reasoning_history_matches_live_reasoning_card_ui() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner.start_local_user("hello".to_string(), false);
    app.transcript_owner.bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        agent_protocol::TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );
    app.transcript_owner.append_reasoning_delta(
        "turn-1".to_string(),
        "r1".to_string(),
        "First paragraph with enough text to wrap.\n\nSecond paragraph for summary.".to_string(),
        false,
    );
    app.transcript_owner.complete_item(
        "turn-1".to_string(),
        "r1".to_string(),
        reasoning(
            "r1",
            "First paragraph with enough text to wrap.\n\nSecond paragraph for summary.",
        ),
        false,
    );

    let committed = app.transcript_owner.committed_history_cells();
    let rendered = committed
        .iter()
        .find(|cell| cell.tone == crate::ui::widgets::history_cell::HistoryTone::Reasoning)
        .expect("reasoning cell should exist")
        .to_lines_with_mode(80)
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("≈ "));
    assert!(rendered.contains("Reasoning"));
    assert!(rendered.contains("│ "));
}

#[test]
fn long_active_reasoning_does_not_expand_viewport_beyond_bottom_pane_stack() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::reasoning(
        "Reasoning",
        "This is a very long reasoning paragraph that should wrap across many lines without forcing the input pane off screen. "
            .repeat(40),
    ));

    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let desired = ChatSurface::desired_viewport_height(&mut app, terminal_area);
    let bottom_only = app.bottom_pane.desired_height(app.current_mode(), 120).max(1);

    assert!(desired > bottom_only);
    assert!(desired <= terminal_area.height);
}

#[test]
fn active_body_height_is_capped_by_remaining_space_above_bottom_pane() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::reasoning(
        "Reasoning",
        "This is a very long reasoning paragraph that should wrap across many lines without forcing the input pane off screen. "
            .repeat(40),
    ));

    let small_terminal = ratatui::layout::Rect::new(0, 0, 120, 20);
    let tall_terminal = ratatui::layout::Rect::new(0, 0, 120, 40);
    let desired_small = ChatSurface::desired_viewport_height(&mut app, small_terminal);
    let desired_tall = ChatSurface::desired_viewport_height(&mut app, tall_terminal);
    let bottom = app.bottom_pane.desired_height(app.current_mode(), 120).max(1);

    assert!(desired_small > bottom);
    assert!(desired_tall > bottom);
    assert!(desired_small <= small_terminal.height);
    assert!(desired_tall <= tall_terminal.height);
    assert!(desired_tall >= desired_small);
}

#[test]
fn active_transcript_tail_keeps_latest_reasoning_visible() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::reasoning(
        "Reasoning",
        "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega"
            .to_string(),
    ));

    let model = build_chat_surface_model(&mut app, 24, 4);
    let ChatSurfaceBody::ActiveCell(active) = model.body else {
        panic!("expected active cell body");
    };
    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("upsilon") || rendered.contains("omega"));
}
