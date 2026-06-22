use super::tool_common::{compact_inline, compact_path, humanize_tool_label};
use super::{ExplorationAggregate, HistoryCell, HistoryTone};
use crate::app::conversation::exploration::{
    is_exploration_command, summarize_exploration_command,
};
use agent_core::{CommandExecutionStatus, RuntimeItem, TurnItemKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandUiKind {
    Exploration,
    Command,
}

pub(super) fn render_active_placeholder(title: &str) -> HistoryCell {
    if classify_command_kind(title) == CommandUiKind::Exploration {
        let command_preview = summarize_exploration_command(title);
        let mut aggregate = ExplorationAggregate::new(command_preview.clone());
        aggregate.inspect_commands = 1;
        return HistoryCell::exploration(
            "Explore workspace",
            command_preview,
            aggregate,
            HistoryTone::Control,
        );
    }

    HistoryCell::exec(
        "Run command",
        summarize_command_head(title),
        Some("running".to_string()),
        HistoryTone::Control,
    )
}

pub(super) fn render_active_runtime_item(item: &RuntimeItem) -> HistoryCell {
    let title = item.title.as_deref().unwrap_or("");
    let mut cell = render_active_placeholder(title);

    if let Some(summary) = item
        .progress
        .as_ref()
        .and_then(|progress| progress.message.clone())
        .or_else(|| item.summary.clone())
        && !summary.trim().is_empty()
        && !matches!(item.kind, TurnItemKind::CommandExecution)
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
    let cwd = compact_path(current_directory, 42);
    let state = match status {
        CommandExecutionStatus::InProgress => "running".to_string(),
        CommandExecutionStatus::Completed => format!("completed{}", exit_suffix(exit_code)),
        CommandExecutionStatus::Declined => "declined".to_string(),
        CommandExecutionStatus::Failed => {
            let reason = detail
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| compact_inline(value, 72))
                .unwrap_or_else(|| "command failed".to_string());
            format!("failed{} — {reason}", exit_suffix(exit_code))
        }
    };

    HistoryCell::exec(
        humanize_tool_label(tool_name),
        summary,
        Some(format!("{state} @ {cwd}")),
        match status {
            CommandExecutionStatus::Failed => HistoryTone::Error,
            CommandExecutionStatus::Declined => HistoryTone::Warning,
            _ => HistoryTone::Control,
        },
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
    if classify_command_kind(command) != CommandUiKind::Exploration {
        return None;
    }

    let command_preview = summarize_exploration_command(command);
    let mut aggregate = ExplorationAggregate::new(command_preview.clone());
    aggregate.inspect_commands = 1;

    Some(HistoryCell::exploration(
        "Explore workspace",
        command_preview,
        aggregate,
        HistoryTone::Control,
    ))
}

fn classify_command_kind(command: &str) -> CommandUiKind {
    if is_exploration_command(command) {
        CommandUiKind::Exploration
    } else {
        CommandUiKind::Command
    }
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
