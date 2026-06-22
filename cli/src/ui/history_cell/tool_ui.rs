use super::HistoryCell;
use super::command;
use super::patch;
use super::search;
use agent_core::{CommandExecutionStatus, RuntimeItem, StructuredToolResult, TurnItemKind};

pub(super) fn render_active_runtime_item(item: &RuntimeItem) -> HistoryCell {
    match item.kind {
        TurnItemKind::CommandExecution => command::render_active_runtime_item(item),
        TurnItemKind::FileChange => patch::render_active_runtime_item(item),
        _ => search::render_active_runtime_item(item),
    }
}

pub(super) fn render_command_execution(
    tool_name: &str,
    command: &str,
    current_directory: &str,
    status: &CommandExecutionStatus,
    exit_code: Option<i32>,
    detail: Option<&str>,
) -> HistoryCell {
    command::render_command_execution(
        tool_name,
        command,
        current_directory,
        status,
        exit_code,
        detail,
    )
}

pub(super) fn render_tool_result(
    tool_name: &str,
    content: &str,
    structured: Option<&StructuredToolResult>,
) -> HistoryCell {
    search::render_tool_result(tool_name, content, structured)
}

pub(crate) fn humanize_tool_label(tool_name: &str) -> String {
    super::tool_common::humanize_tool_label(tool_name)
}
