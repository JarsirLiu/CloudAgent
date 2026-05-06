use crate::app::core::transcript_owner::TranscriptOwner;
use crate::app::conversation::facade as conversation_facade;
use crate::app::TuiApp;
use crate::ui::chat_surface_model::{ChatSurfaceBody, build_chat_surface_model};
use agent_protocol::CommandExecutionStatus;
use agent_protocol::{ConversationTurn, TranscriptItem, TurnState};
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
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();

    assert_eq!(pending, vec!["first", "done", "second"]);
    let visible = owner
        .live_cells()
        .iter()
        .map(|cell| cell.body().to_string())
        .collect::<Vec<_>>();
    assert_eq!(visible, vec!["thinking"]);
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
