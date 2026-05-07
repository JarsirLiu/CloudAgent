use crate::app::core::transcript_owner::TranscriptOwner;
use crate::app::conversation::facade as conversation_facade;
use crate::app::TuiApp;
use crate::ui::chat_surface_model::{ChatSurfaceBody, build_chat_surface_model};
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
        pending.iter().any(|entry| entry.contains("Read file|read file")),
        "pending: {pending:?}"
    );
    assert!(
        pending.iter().all(|entry| !entry.contains("Read file|running")),
        "pending: {pending:?}"
    );
}

#[test]
fn adjacent_tool_results_group_by_label() {
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
        .map(|cell| {
            (
                cell.label().to_string(),
                cell.body().to_string(),
                cell.children().map(|children| children.len()).unwrap_or(0),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        pending,
        vec![
            ("you".to_string(), "hello".to_string(), 0),
            ("Read file".to_string(), "read file".to_string(), 0),
            (
                "Search workspace".to_string(),
                "matched 3 hits in 2 files".to_string(),
                0,
            ),
            ("Reasoning".to_string(), "thinking".to_string(), 0),
            ("Read file".to_string(), "read file".to_string(), 0),
        ]
    );
}

#[test]
fn adjacent_same_tool_results_merge_into_group() {
    let mut owner = TranscriptOwner::default();
    let history = vec![turn(
        "turn-1",
        TurnState::Completed,
        vec![
            user("u1", "hello"),
            search_workspace_result("sw1", "active_turn"),
            search_workspace_result("sw2", "history_cell"),
        ],
    )];

    owner.rebuild_from_history_snapshot(&history, false);

    let pending = owner.pending_history_cells().iter().collect::<Vec<_>>();

    assert_eq!(pending.len(), 2);
    assert_eq!(pending[1].label(), "Search workspace");
    assert_eq!(pending[1].body(), "searched workspace 2 times");
    assert_eq!(pending[1].children().map(|children| children.len()), Some(2));
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
    app.run_state.history_loaded = true;

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
    app.run_state.history_loaded = true;

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
    app.run_state.history_loaded = true;

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
    app.run_state.history_loaded = true;

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
