use super::render_command_execution;
use crate::ui::history_cell::{HistoryTone, RenderContext, render_active_runtime_item};
use agent_core::conversation::TranscriptItem;
use agent_core::{
    CommandExecutionStatus, RuntimeItem, RuntimeItemProgress, RuntimeItemStatus, TurnItemKind,
};

#[test]
fn exploration_command_renders_as_workspace_exploration() {
    let cell = render_command_execution(
        "exec_command",
        "ls src",
        "D:\\work",
        &CommandExecutionStatus::Completed,
        Some(0),
        None,
    );

    assert_eq!(cell.label(), "Explore workspace");
    assert_eq!(cell.tone, HistoryTone::Control);
    assert!(cell.body().contains("ls src"));
}

#[test]
fn failed_command_includes_exit_code_and_reason() {
    let cell = render_command_execution(
        "exec_command",
        "cargo test",
        "D:\\work\\repo",
        &CommandExecutionStatus::Failed,
        Some(101),
        Some("command failed: missing import"),
    );

    assert_eq!(cell.label(), "Run command");
    assert!(cell.detail().as_deref().unwrap_or("").contains("failed"));
    assert!(cell.detail().as_deref().unwrap_or("").contains("exit 101"));
    assert!(cell.detail().as_deref().unwrap_or("").contains("missing import"));
    assert_eq!(cell.tone, HistoryTone::Error);
}

#[test]
fn write_stdin_poll_with_no_output_is_hidden() {
    let item = RuntimeItem {
        id: "tool-1".to_string(),
        call_id: None,
        kind: TurnItemKind::CommandExecution,
        title: Some("write_stdin".to_string()),
        status: RuntimeItemStatus::InProgress,
        summary: None,
        tool_identity: None,
        structured: None,
        progress: Some(RuntimeItemProgress {
            message: None,
            completed: None,
            total: None,
            unit: None,
        }),
        metrics: None,
    };

    let cell = render_active_runtime_item(&item);

    assert_eq!(cell.label(), "Run command");
    assert!(cell.body().contains("running"));
}
