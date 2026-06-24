use super::tool_common::{compact_inline, compact_path, humanize_tool_label, runtime_summary};
use super::{ExplorationAggregate, HistoryCell, HistoryTone};
use crate::app::conversation::exploration::{
    is_exploration_command, summarize_exploration_command,
};
use agent_core::{CommandExecutionStatus, RuntimeItem, TurnItemKind};

pub(super) fn render_active_placeholder(title: &str) -> HistoryCell {
    if is_exploration_command(title) {
        return render_exploration_placeholder(title);
    }

    render_command_placeholder(title)
}

pub(super) fn render_active_runtime_item(item: &RuntimeItem) -> HistoryCell {
    let title = item.title.as_deref().unwrap_or("");
    let mut cell = render_active_placeholder(title);

    if !matches!(item.kind, TurnItemKind::CommandExecution)
        && let Some(summary) = runtime_summary(item)
    {
        cell.replace_body(summary);
    }

    cell
}

pub(super) fn render_command_execution(
    tool_name: &str,
    command: &str,
    current_directory: &str,
    status: &CommandExecutionStatus,
    exit_code: Option<i32>,
    detail: Option<&str>,
) -> HistoryCell {
    if is_empty_stdin_poll_result(tool_name, status, detail) {
        return HistoryCell::info("", "", HistoryTone::Meta);
    }
    if let Some(exploration) = render_exploration_command(command) {
        return exploration;
    }

    let summary = summarize_command_head(command);
    let state = command_state(status, exit_code, detail);
    let tone = command_tone(status);
    let cwd = compact_path(current_directory, 42);
    HistoryCell::exec(
        humanize_tool_label(tool_name),
        summary,
        Some(format!("{state} @ {cwd}")),
        tone,
    )
}

fn is_empty_stdin_poll_result(
    tool_name: &str,
    status: &CommandExecutionStatus,
    detail: Option<&str>,
) -> bool {
    tool_name == "write_stdin"
        && !matches!(
            status,
            CommandExecutionStatus::Failed | CommandExecutionStatus::Declined
        )
        && detail.is_none_or(|value| value.trim().is_empty())
}

fn render_exploration_command(command: &str) -> Option<HistoryCell> {
    if !is_exploration_command(command) {
        return None;
    }

    Some(render_exploration_placeholder(command))
}

fn render_exploration_placeholder(command: &str) -> HistoryCell {
    let command_preview = summarize_exploration_command(command);
    let mut aggregate = ExplorationAggregate::new(command_preview.clone());
    aggregate.inspect_commands = 1;

    HistoryCell::exploration(
        "Explore workspace",
        command_preview,
        aggregate,
        HistoryTone::Control,
    )
}

fn render_command_placeholder(command: &str) -> HistoryCell {
    HistoryCell::exec(
        "Run command",
        summarize_command_head(command),
        Some("running".to_string()),
        HistoryTone::Control,
    )
}
fn command_state(
    status: &CommandExecutionStatus,
    exit_code: Option<i32>,
    detail: Option<&str>,
) -> String {
    match status {
        CommandExecutionStatus::InProgress => "running".to_string(),
        CommandExecutionStatus::Completed => format!("completed{}", exit_suffix(exit_code)),
        CommandExecutionStatus::Declined => "declined".to_string(),
        CommandExecutionStatus::Failed => {
            let reason = command_failure_reason(detail);
            format!("failed{} 鈥?{reason}", exit_suffix(exit_code))
        }
    }
}

fn command_tone(status: &CommandExecutionStatus) -> HistoryTone {
    match status {
        CommandExecutionStatus::Failed => HistoryTone::Error,
        CommandExecutionStatus::Declined => HistoryTone::Warning,
        _ => HistoryTone::Control,
    }
}

fn command_failure_reason(detail: Option<&str>) -> String {
    detail
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| compact_inline(value, 72))
        .unwrap_or_else(|| "command failed".to_string())
}

fn exit_suffix(exit_code: Option<i32>) -> String {
    exit_code
        .map(|code| format!(" (exit {code})"))
        .unwrap_or_default()
}

fn summarize_command_head(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return "empty command".to_string();
    }

    let normalized = trimmed.replace('\n', " ");
    if let Some((_, rhs)) = normalized.rsplit_once("&&") {
        return compact_inline(rhs.trim(), 64);
    }
    compact_inline(normalized.trim(), 64)
}
