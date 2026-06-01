use crate::app::TuiApp;
use crate::app::conversation::actions::execute_server_action;
use crate::app::conversation::facade as conversation_facade;
use crate::app::core::transcript_owner::TranscriptOwner;
use crate::app::runtime::display::should_show_welcome;
use crate::ui::chat_surface::ChatSurface;
use crate::ui::chat_surface_model::{ChatSurfaceBody, build_chat_surface_model};
use agent_core::{
    CommandExecutionStatus, ConversationTurn, InputItem, ReadFileEntry, ReadFileStatus,
    SearchWorkspaceMode, SearchWorkspaceOperation, SearchWorkspaceStatus, StructuredToolResult,
    TranscriptItem, TurnItemKind, TurnState,
};
use std::path::PathBuf;

fn user(id: &str, text: &str) -> TranscriptItem {
    TranscriptItem::UserMessage {
        id: id.to_string(),
        content: vec![InputItem::Text {
            text: text.to_string(),
        }],
    }
}

fn local_input(text: &str) -> Vec<InputItem> {
    vec![InputItem::Text {
        text: text.to_string(),
    }]
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
    command_with_status(id, command, CommandExecutionStatus::Completed)
}

fn command_with_status(id: &str, command: &str, status: CommandExecutionStatus) -> TranscriptItem {
    TranscriptItem::CommandExecution {
        id: id.to_string(),
        tool_name: "exec_command".to_string(),
        command: command.to_string(),
        current_directory: "D:\\learn\\gifti\\cloudagent".to_string(),
        status: status.clone(),
        exit_code: matches!(status, CommandExecutionStatus::Completed).then_some(0),
        output: Some(String::new()),
        duration_ms: Some(1),
        summary: command.to_string(),
    }
}

fn test_app() -> TuiApp {
    TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    )
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
    owner.start_local_user(local_input("hello"), false);

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
    assert_eq!(
        owner.active_cell().map(|cell| cell.body()),
        Some("second notice")
    );
}

#[test]
fn transcript_owner_rebuilds_history_and_keeps_only_live_tail_active() {
    let mut owner = TranscriptOwner::default();
    let history = vec![
        turn(
            "turn-1",
            TurnState::Completed,
            vec![user("u1", "first"), agent("a1", "done")],
        ),
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
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
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
        TurnItemKind::CommandExecution,
        Some("rg *".to_string()),
        false,
    );

    let visible = owner
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert!(visible.is_empty());

    owner.complete_item(
        "turn-1".to_string(),
        "c1".to_string(),
        command("c1", "rg *"),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
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
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
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
        TurnItemKind::CommandExecution,
        Some("rg esc".to_string()),
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
    assert!(visible.is_empty());
}

#[test]
fn completed_agent_message_consolidates_after_item_boundary() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "visible prefix".to_string(),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "c1".to_string(),
        TurnItemKind::CommandExecution,
        Some("rg esc".to_string()),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent("a1", "visible prefix and final text"),
        false,
    );

    let committed = owner
        .committed_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(committed, "hellovisible prefix and final text");
}

#[test]
fn completed_agent_message_consolidates_provisional_cells_by_item_id() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "first stable\n\n".to_string(),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "c1".to_string(),
        TurnItemKind::CommandExecution,
        Some("rg esc".to_string()),
        false,
    );
    owner.complete_item(
        "turn-1".to_string(),
        "c1".to_string(),
        command("c1", "rg esc"),
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "second stable\n\n".to_string(),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent("a1", "first stable\n\nsecond stable\n\nfinal tail"),
        false,
    );

    let committed = owner
        .committed_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        committed,
        vec![
            "hello".to_string(),
            "first stable\n\nsecond stable\n\nfinal tail".to_string(),
            "ran 1 inspect command".to_string(),
        ]
    );
}

#[test]
fn completed_agent_message_without_stream_appends_instead_of_replacing_previous_agent() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent("a1", "first answer"),
        false,
    );
    owner.complete_item(
        "turn-1".to_string(),
        "a2".to_string(),
        agent("a2", "second answer"),
        false,
    );

    let committed = owner
        .committed_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(committed, vec!["hello", "first answersecond answer"]);
}

#[test]
fn runtime_non_agent_cells_do_not_become_stream_continuations() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
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
        TurnItemKind::CommandExecution,
        Some("Run command".to_string()),
        false,
    );
    owner.complete_item(
        "turn-1".to_string(),
        "c1".to_string(),
        command("c1", "rg cli"),
        false,
    );
    owner.complete_turn("turn-1".to_string(), false);

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
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "toolcall-1".to_string(),
        TurnItemKind::ToolCall,
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
        TurnItemKind::AssistantMessage,
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
        pending
            .iter()
            .any(|entry| entry.contains("Explored workspace|read 1 file")),
        "pending: {pending:?}"
    );
    assert!(
        pending
            .iter()
            .all(|entry| !entry.contains("Read file|running")),
        "pending: {pending:?}"
    );
}

#[test]
fn parallel_toolcall_placeholders_do_not_commit_running_cards() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);

    owner.start_item(
        "turn-1".to_string(),
        "toolcall-1".to_string(),
        TurnItemKind::ToolCall,
        Some("read_file".to_string()),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "toolcall-2".to_string(),
        TurnItemKind::ToolCall,
        Some("read_file".to_string()),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "toolcall-3".to_string(),
        TurnItemKind::ToolCall,
        Some("read_file".to_string()),
        false,
    );
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );

    let pending = owner
        .pending_history_cells()
        .iter()
        .map(|cell| format!("{}|{}", cell.label(), cell.body()))
        .collect::<Vec<_>>();

    assert!(
        pending
            .iter()
            .all(|entry| !entry.contains("Read file|running")),
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
    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
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
            vec![
                user("u1", "hello"),
                reasoning("r1", "thinking from snapshot"),
            ],
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
    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
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
    let ChatSurfaceBody::Transcript(active) = model.body else {
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
    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
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
    let ChatSurfaceBody::Transcript(active) = model.body else {
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
fn live_reasoning_tail_shows_latest_lines_without_history_collapse() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );
    let text = (0..30)
        .map(|line| format!("reasoning line {line:02}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    app.transcript_owner.append_reasoning_delta(
        "turn-1".to_string(),
        "r1".to_string(),
        text,
        false,
    );

    let model = build_chat_surface_model(&mut app, 80, 8);
    let ChatSurfaceBody::Transcript(active) = model.body else {
        panic!("expected active cell body");
    };
    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("reasoning line 29"));
    assert!(!rendered.contains("… +"));
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
    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
        Some("Reasoning".to_string()),
        false,
    );

    let model = build_chat_surface_model(&mut app, 80, 12);
    let ChatSurfaceBody::Transcript(active) = model.body else {
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
fn committed_history_without_active_cell_renders_in_transcript_body() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner
        .start_local_user(local_input("hello"), false);

    let model = build_chat_surface_model(&mut app, 80, 20);
    let ChatSurfaceBody::Transcript(active) = model.body else {
        panic!("expected active cell body");
    };

    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert!(rendered.iter().any(|line| line.contains("hello")));
    assert_eq!(model.body_height, 1);
}

#[test]
fn committed_history_without_active_cell_counts_transcript_height() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner
        .start_local_user(local_input("hello"), false);

    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let desired = ChatSurface::desired_viewport_height(&mut app, terminal_area);
    let bottom_only = app
        .bottom_pane
        .desired_height(app.current_mode(), 120)
        .max(1);

    assert_eq!(desired, bottom_only.saturating_add(3));
}

#[test]
fn slash_completion_expands_bottom_pane_as_single_layout_region() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::agent(
        "cloudagent",
        "streaming response stays stable while the command list is open",
        crate::ui::widgets::history_cell::HistoryFormat::Markdown,
    ));

    let terminal_area = ratatui::layout::Rect::new(0, 0, 120, 40);
    let before = ChatSurface::desired_viewport_height(&mut app, terminal_area);
    let bottom_height_before = app
        .bottom_pane
        .desired_height(app.current_mode(), terminal_area.width);

    let _ = app.bottom_pane.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('/'),
        crossterm::event::KeyModifiers::NONE,
    ));

    let after = ChatSurface::desired_viewport_height(&mut app, terminal_area);
    let bottom_height_after = app
        .bottom_pane
        .desired_height(app.current_mode(), terminal_area.width);

    assert!(bottom_height_after > bottom_height_before);
    assert_eq!(
        after,
        before.saturating_add(bottom_height_after - bottom_height_before)
    );
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
fn reset_local_view_clears_app_owned_transcript() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.reset_local_view();

    assert!(!app.transcript_owner.has_transcript_content());
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
fn server_notice_uses_transient_status_banner_instead_of_transcript_cell() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::PushNoticeCell {
            label: "conversation".to_string(),
            message: "Deleted conversation `draft-1778341755002`".to_string(),
            level: crate::state::NoticeLevel::Info,
        },
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("Deleted conversation `draft-1778341755002`")
    );
    assert_eq!(
        status.live_banner_level,
        Some(crate::state::NoticeLevel::Info)
    );
    assert!(app.transcript_owner.active_cell().is_none());
    assert!(!app.transcript_owner.has_transcript_content());

    app.bottom_pane.expire_transient_notice_for_test();
    assert!(app.bottom_pane.handle_tick());

    let cleared = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(cleared.live_banner, None);
}

#[test]
fn server_error_uses_transient_status_banner_instead_of_transcript_cell() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::PushErrorCell("interrupt requested".to_string()),
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(status.live_banner.as_deref(), Some("interrupt requested"));
    assert_eq!(
        status.live_banner_level,
        Some(crate::state::NoticeLevel::Error)
    );
    assert!(app.transcript_owner.active_cell().is_none());
    assert!(!app.transcript_owner.has_transcript_content());
}

#[test]
fn server_request_prompt_uses_warning_status_banner_instead_of_transcript_cell() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::ShowServerRequestPrompt {
            request_id: agent_protocol::RequestId::String("req-1".to_string()),
            title: "approval".to_string(),
            detail: "detail".to_string(),
            notice: "Command approval required".to_string(),
        },
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("Command approval required")
    );
    assert_eq!(
        status.live_banner_level,
        Some(crate::state::NoticeLevel::Warn)
    );
    assert!(app.transcript_owner.active_cell().is_none());
    assert!(!app.transcript_owner.has_transcript_content());
}

#[test]
fn command_output_delta_updates_status_without_transcript_history() {
    let mut app = test_app();
    app.sync_frontend_mode(agent_protocol::FrontendMode::Running);
    app.bottom_pane.on_turn_started();

    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::StartActiveTurnItem {
            turn_id: "turn-1".to_string(),
            item_id: "cmd-1".to_string(),
            kind: TurnItemKind::CommandExecution,
            title: Some("rg TODO".to_string()),
        },
    );
    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::AppendCommandOutputDelta {
            item_id: "cmd-1".to_string(),
            delta: "src/lib.rs: TODO".to_string(),
        },
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: rg TODO · src/lib.rs: TODO")
    );
    assert!(app.transcript_owner.active_cell().is_none());
    assert!(app.transcript_owner.pending_history_cells().is_empty());
}

#[test]
fn in_progress_command_completion_keeps_status_until_final_completion() {
    let mut app = test_app();
    app.sync_frontend_mode(agent_protocol::FrontendMode::Running);
    app.bottom_pane.on_turn_started();

    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::StartActiveTurnItem {
            turn_id: "turn-1".to_string(),
            item_id: "cmd-1".to_string(),
            kind: TurnItemKind::CommandExecution,
            title: Some("slow command".to_string()),
        },
    );
    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::CompleteActiveTurnItem {
            turn_id: "turn-1".to_string(),
            item_id: "cmd-1".to_string(),
            item: command_with_status("cmd-1", "slow command", CommandExecutionStatus::InProgress),
        },
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(
        status.live_banner.as_deref(),
        Some("running command: slow command")
    );

    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::CompleteActiveTurnItem {
            turn_id: "turn-1".to_string(),
            item_id: "cmd-1".to_string(),
            item: command("cmd-1", "slow command"),
        },
    );

    let status = app.bottom_pane.build_status_view_model(&app);
    assert_eq!(status.live_banner, None);
    let committed = app
        .transcript_owner
        .pending_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(committed, vec!["slow command"]);
}

#[test]
fn completed_streamed_agent_item_survives_turn_completion() {
    let mut app = test_app();
    app.sync_frontend_mode(agent_protocol::FrontendMode::Running);
    app.bottom_pane.on_turn_started();

    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::BindActiveTurn("turn-1".to_string()),
    );
    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::StartActiveTurnItem {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            kind: TurnItemKind::AssistantMessage,
            title: None,
        },
    );
    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::AppendActiveAgentDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            delta: "visible during stream\n\n".to_string(),
        },
    );
    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::CompleteActiveTurnItem {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            item: agent(
                "assistant:1",
                "visible during stream\n\nfinal line one\nfinal line two",
            ),
        },
    );
    execute_server_action(
        &mut app,
        crate::state::reducer::ServerAction::TurnDispatch(
            crate::state::reducer::TurnDispatch::Completed,
        ),
    );

    let committed = app
        .transcript_owner
        .committed_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(committed.contains("visible during stream"));
    assert!(committed.contains("final line one"));
    assert!(committed.contains("final line two"));
    assert!(app.transcript_owner.active_cell().is_none());
}

#[test]
fn completed_streamed_agent_item_consolidates_canonical_transcript() {
    let mut owner = TranscriptOwner::default();

    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "assistant:1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "assistant:1".to_string(),
        "stable paragraph\n\nlive tail".to_string(),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "assistant:1".to_string(),
        agent("assistant:1", "stable paragraph\n\nlive tail\nfinal line"),
        false,
    );
    owner.complete_turn("turn-1".to_string(), false);

    let body = owner
        .committed_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(body.contains("stable paragraph"));
    assert!(body.contains("live tail"));
    assert!(body.contains("final line"));
    assert!(owner.active_cell().is_none());
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

    app.prepare_submitted_turn(&[InputItem::Text {
        text: "hello".to_string(),
    }]);
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
fn failed_turn_restores_submitted_text_to_composer() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    let submitted = vec![InputItem::Text {
        text: "continue from this draft".to_string(),
    }];
    app.prepare_submitted_turn(&submitted);
    app.apply_turn_dispatch(crate::state::reducer::TurnDispatch::Failed {
        error: "network error".to_string(),
    });

    let lines =
        app.bottom_pane
            .render_lines_for_test(agent_protocol::FrontendMode::Idle, "", "", 80);
    let rendered = lines
        .0
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("continue from this draft"));
    assert!(app.can_submit_turn());
    assert!(app.run_state.pending_submitted_input.is_none());
}

#[test]
fn failed_turn_restores_submitted_images_to_composer() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );
    let image_path = std::env::temp_dir().join("cloudagent-failed-turn-restore.png");
    image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
        .save(&image_path)
        .expect("save temp image");

    let submitted = vec![
        InputItem::Text {
            text: "check this".to_string(),
        },
        InputItem::Image {
            source: agent_core::AttachmentRef::LocalPath {
                path: image_path.display().to_string(),
            },
            detail: None,
            alt: None,
        },
    ];
    app.prepare_submitted_turn(&submitted);
    app.apply_turn_dispatch(crate::state::reducer::TurnDispatch::Failed {
        error: "provider rejected message".to_string(),
    });

    let lines =
        app.bottom_pane
            .render_lines_for_test(agent_protocol::FrontendMode::Idle, "", "", 80);
    let rendered = lines
        .0
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("check this"));
    assert!(rendered.contains("[Image #1]"));
}

#[test]
fn failed_turn_restores_non_image_input_semantics_as_editable_text() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    let submitted = vec![
        InputItem::File {
            source: agent_core::AttachmentRef::LocalPath {
                path: "D:\\tmp\\spec.pdf".to_string(),
            },
            mime_type: Some("application/pdf".to_string()),
            name: Some("spec.pdf".to_string()),
        },
        InputItem::Mention {
            name: "workspace".to_string(),
            path: "D:\\learn\\gifti\\cloudagent".to_string(),
        },
        InputItem::Skill {
            name: "browser-use".to_string(),
            path: "plugin://browser-use".to_string(),
        },
    ];
    app.prepare_submitted_turn(&submitted);
    app.apply_turn_dispatch(crate::state::reducer::TurnDispatch::Failed {
        error: "temporary upstream failure".to_string(),
    });

    let lines =
        app.bottom_pane
            .render_lines_for_test(agent_protocol::FrontendMode::Idle, "", "", 100);
    let rendered = lines
        .0
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("[Attachment: spec.pdf (application/pdf)]"));
    assert!(rendered.contains("@workspace (D:\\learn\\gifti\\cloudagent)"));
    assert!(rendered.contains("$browser-use"));
}

#[test]
fn failed_turn_without_pending_draft_does_not_claim_restore() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);

    app.apply_turn_dispatch(crate::state::reducer::TurnDispatch::Failed {
        error: "worker app server closed unexpectedly".to_string(),
    });

    let active = app
        .transcript_owner
        .active_cell()
        .map(|cell| cell.body().to_string())
        .unwrap_or_default();
    assert!(active.contains("failed: worker app server closed unexpectedly"));
    assert!(!active.contains("draft restored for retry"));
    assert!(app.can_submit_turn());
}

#[test]
fn agent_stream_commits_complete_lines_and_keeps_tail_live() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );

    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "stable line\n\nlive tail".to_string(),
        false,
    );

    let live = owner
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(live, vec!["live tail"]);

    let pending = owner
        .pending_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert!(pending.iter().any(|body| body.contains("stable line")));
    assert!(!pending.iter().any(|body| body.contains("live tail")));
}

#[test]
fn completing_streamed_agent_message_only_commits_remaining_tail() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "stable line\n\nlive tail".to_string(),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent("a1", "stable line\n\nlive tail"),
        false,
    );

    let pending = owner
        .pending_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    let stable_count = pending
        .iter()
        .filter(|body| body.contains("stable line"))
        .count();
    let tail_count = pending
        .iter()
        .filter(|body| body.contains("live tail"))
        .count();
    assert_eq!(stable_count, 1);
    assert_eq!(tail_count, 1);
}

#[test]
fn completing_unflushed_stream_keeps_canonical_transcript() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "stable line\n\nlive tail".to_string(),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent("a1", "stable line\n\nlive tail"),
        false,
    );

    let committed = owner
        .committed_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(committed, vec!["hello", "stable line\n\nlive tail"]);
}

#[test]
fn completing_partially_streamed_message_consolidates_source() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "first stable\n\n".to_string(),
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "second stable\n\nlive tail".to_string(),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent(
            "a1",
            "first stable\n\nsecond stable\n\nlive tail\nfinal text",
        ),
        false,
    );

    let committed = owner
        .committed_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        committed,
        vec![
            "hello".to_string(),
            "first stable\n\nsecond stable\n\nlive tail\nfinal text".to_string()
        ]
    );
}

#[test]
fn completing_streamed_agent_message_commits_source_backed_text() {
    let mut owner = TranscriptOwner::default();
    owner.start_local_user(local_input("hello"), false);
    owner.bind_turn_id("turn-1".to_string(), false);
    owner.start_item(
        "turn-1".to_string(),
        "a1".to_string(),
        TurnItemKind::AssistantMessage,
        None,
        false,
    );
    owner.append_agent_delta(
        "turn-1".to_string(),
        "a1".to_string(),
        "visible prefix".to_string(),
        false,
    );

    owner.complete_item(
        "turn-1".to_string(),
        "a1".to_string(),
        agent("a1", "visible prefix and final text"),
        false,
    );

    let rendered = owner
        .pending_history_cells()
        .into_iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(rendered, "hellovisible prefix and final text");
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
    let ChatSurfaceBody::Transcript(active) = model.body else {
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
    let ChatSurfaceBody::Transcript(active) = model.body else {
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
    app.transcript_owner
        .start_local_user(local_input("hello"), false);
    app.transcript_owner
        .bind_turn_id("turn-1".to_string(), false);
    app.transcript_owner.start_item(
        "turn-1".to_string(),
        "r1".to_string(),
        TurnItemKind::Reasoning,
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
    let bottom_only = app
        .bottom_pane
        .desired_height(app.current_mode(), 120)
        .max(1);

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
    let bottom = app
        .bottom_pane
        .desired_height(app.current_mode(), 120)
        .max(1);

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
    let ChatSurfaceBody::Transcript(active) = model.body else {
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

#[test]
fn active_transcript_manual_scroll_is_preserved_across_new_content() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::agent(
        "assistant",
        (0..10)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n"),
        crate::ui::widgets::history_cell::HistoryFormat::PlainText,
    ));

    let initial = build_chat_surface_model(&mut app, 80, 4);
    let ChatSurfaceBody::Transcript(initial_active) = initial.body else {
        panic!("expected active cell body");
    };
    assert_eq!(initial_active.lines.len(), 10);
    assert_eq!(app.transcript_scroll.top_row_for_render(10, 4), 6);

    assert!(
        app.transcript_scroll
            .handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::PageUp,
                crossterm::event::KeyModifiers::NONE,
            ))
    );

    let scrolled = build_chat_surface_model(&mut app, 80, 4);
    let ChatSurfaceBody::Transcript(scrolled_active) = scrolled.body else {
        panic!("expected active cell body");
    };
    assert_eq!(scrolled_active.lines.len(), 10);
    assert_eq!(app.transcript_scroll.top_row_for_render(10, 4), 3);

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::agent(
        "assistant",
        (0..14)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n"),
        crate::ui::widgets::history_cell::HistoryFormat::PlainText,
    ));

    let updated = build_chat_surface_model(&mut app, 80, 4);
    let ChatSurfaceBody::Transcript(updated_active) = updated.body else {
        panic!("expected active cell body");
    };
    assert_eq!(updated_active.lines.len(), 14);
    assert_eq!(app.transcript_scroll.top_row_for_render(14, 4), 3);
}

#[test]
fn active_body_height_tracks_wrapped_physical_rows_during_streaming() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::agent(
        "assistant",
        "This is a single long streaming paragraph that should wrap into multiple physical rows inside the active viewport instead of being treated like a single logical line."
            .to_string(),
        crate::ui::widgets::history_cell::HistoryFormat::PlainText,
    ));

    let model = build_chat_surface_model(&mut app, 28, 20);
    let ChatSurfaceBody::Transcript(active) = model.body else {
        panic!("expected active cell body");
    };

    assert!(active.lines.len() > 1, "wrapped lines: {:?}", active.lines);
    assert_eq!(model.body_height, active.lines.len() as u16);
}

#[test]
fn active_body_height_does_not_add_phantom_margin_rows() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::agent(
        "assistant",
        "one visible line".to_string(),
        crate::ui::widgets::history_cell::HistoryFormat::PlainText,
    ));

    let model = build_chat_surface_model(&mut app, 80, 20);
    let ChatSurfaceBody::Transcript(active) = model.body else {
        panic!("expected active cell body");
    };

    assert_eq!(active.lines.len(), 1);
    assert_eq!(model.body_height, 1);
}

#[test]
fn active_transcript_tail_ignores_trailing_blank_stream_rows() {
    let mut app = TuiApp::new(
        "default".to_string(),
        "test",
        PathBuf::from("D:\\learn\\gifti\\cloudagent"),
        PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
        false,
        "ReadOnly".to_string(),
    );

    app.push_live_cell(crate::ui::widgets::history_cell::HistoryCell::agent(
        "assistant",
        "line 0\nline 1\nline 2\n\n\n".to_string(),
        crate::ui::widgets::history_cell::HistoryFormat::PlainText,
    ));

    let model = build_chat_surface_model(&mut app, 80, 2);
    let ChatSurfaceBody::Transcript(active) = model.body else {
        panic!("expected active cell body");
    };

    let rendered = active
        .lines
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    assert_eq!(model.body_height, 3);
    assert_eq!(rendered.last().map(|line| line.trim()), Some("line 2"));
    assert_eq!(app.transcript_scroll.top_row_for_render(3, 2), 1);
}

#[test]
fn transcript_surface_uses_centered_width_metrics() {
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let metrics = ChatSurface::transcript_render_metrics_for_area(area);

    assert_eq!(metrics.width, 116);
    assert_eq!(metrics.left_padding, 2);
}
