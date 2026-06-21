use super::tool_operation::{ToolOperation, classify_structured_result, classify_tool_name};
use super::{ExplorationAggregate, HistoryCell, HistoryTone};
use crate::app::conversation::exploration::{
    is_exploration_command, summarize_exploration_command,
};
use crate::tool_identity::WEB_SEARCH_TOOL_NAME;
use agent_core::{
    CommandExecutionStatus, SearchWorkspaceMode, SearchWorkspaceStatus, StructuredToolResult,
    TurnItemKind, WriteFileStatus,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolUiKind {
    Exploration(ExplorationKind),
    Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExplorationKind {
    WorkspaceCommand,
    ReadFile,
    SearchWorkspace,
    ReadDirectory,
    FileInfo,
}

pub(super) fn render_active_placeholder(kind: TurnItemKind, title: &str) -> HistoryCell {
    match kind {
        TurnItemKind::CommandExecution
            if classify_command_kind(title)
                == ToolUiKind::Exploration(ExplorationKind::WorkspaceCommand) =>
        {
            let command_preview = summarize_exploration_command(title);
            let mut aggregate = ExplorationAggregate::new(command_preview.clone());
            aggregate.inspect_commands = 1;
            HistoryCell::exploration(
                exploration_label(ExplorationKind::WorkspaceCommand),
                command_preview,
                aggregate,
                HistoryTone::Control,
            )
        }
        TurnItemKind::CommandExecution => HistoryCell::exec(
            "Run command",
            summarize_command_head(title),
            Some("running".to_string()),
            HistoryTone::Control,
        ),
        TurnItemKind::ToolCall => HistoryCell::info(
            humanize_tool_label(title),
            "running".to_string(),
            HistoryTone::Control,
        ),
        TurnItemKind::ToolResult => HistoryCell::info(
            humanize_tool_label(title),
            "running".to_string(),
            HistoryTone::Control,
        ),
        _ => HistoryCell::info(
            title.to_string(),
            "running".to_string(),
            HistoryTone::Control,
        ),
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
            format!("failed{} • {reason}", exit_suffix(exit_code))
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

pub(super) fn render_tool_result(
    tool_name: &str,
    content: &str,
    structured: Option<&StructuredToolResult>,
) -> HistoryCell {
    if let Some(StructuredToolResult::CommandExecution {
        command,
        current_directory,
        status,
        exit_code,
        output,
        ..
    }) = structured
    {
        return render_command_execution(
            tool_name,
            command,
            current_directory,
            status,
            *exit_code,
            output.as_deref(),
        );
    }
    if let Some(StructuredToolResult::ReadFile {
        path,
        total_chars,
        read,
        ..
    }) = structured
    {
        let display_path = compact_path(path, 56);
        let range_suffix = format_line_range(read.start_line, read.end_line);
        let detail = format!(
            "{}{} • {} chars{}",
            display_path,
            range_suffix,
            total_chars,
            if read.truncated { " truncated" } else { "" }
        );
        let mut aggregate = ExplorationAggregate::new(detail);
        aggregate.read_files = 1;
        return HistoryCell::exploration(
            exploration_label(ExplorationKind::ReadFile),
            "read 1 file".to_string(),
            aggregate,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::SearchWorkspace {
        mode,
        status,
        query,
        file_count,
        match_count,
        truncated,
        ..
    }) = structured
    {
        let status_suffix = match status {
            SearchWorkspaceStatus::Active => "",
            SearchWorkspaceStatus::Closed => " closed",
            SearchWorkspaceStatus::NotFound => " missing",
        };
        let truncation = if *truncated { " truncated" } else { "" };
        let summary = match mode {
            SearchWorkspaceMode::Files => {
                format!("found {file_count} files{truncation}{status_suffix}")
            }
            SearchWorkspaceMode::Text => {
                format!(
                    "matched {match_count} hits in {file_count} files{truncation}{status_suffix}"
                )
            }
        };
        let detail = match mode {
            SearchWorkspaceMode::Files => {
                format!("file search `{}`", compact_inline(query, 48))
            }
            SearchWorkspaceMode::Text => {
                format!("text search `{}`", compact_inline(query, 48))
            }
        };
        let mut aggregate = ExplorationAggregate::new(detail);
        aggregate.searches = 1;
        return HistoryCell::exploration(
            exploration_label(ExplorationKind::SearchWorkspace),
            summary,
            aggregate,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::ReadDirectory {
        path,
        entry_count,
        truncated,
        ..
    }) = structured
    {
        let mut aggregate = ExplorationAggregate::new(format!(
            "{} • {} entries{}",
            compact_path(path, 56),
            entry_count,
            if *truncated { " truncated" } else { "" }
        ));
        aggregate.listed_directories = 1;
        return HistoryCell::exploration(
            exploration_label(ExplorationKind::ReadDirectory),
            "listed 1 directory".to_string(),
            aggregate,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::GetMetadata {
        path,
        size,
        exists,
        is_file,
        ..
    }) = structured
    {
        if *exists {
            let kind = if *is_file { "file" } else { "directory" };
            let mut aggregate = ExplorationAggregate::new(format!(
                "metadata {} • {kind} ({size} bytes)",
                compact_path(path, 56)
            ));
            aggregate.metadata_reads = 1;
            return HistoryCell::exploration(
                exploration_label(ExplorationKind::FileInfo),
                "checked 1 path".to_string(),
                aggregate,
                HistoryTone::Control,
            );
        }
        return HistoryCell::info(
            humanize_tool_label(tool_name),
            format!("metadata missing — {}", compact_path(path, 56)),
            HistoryTone::Warning,
        );
    }
    if let Some(StructuredToolResult::EditFile {
        changed_paths,
        files_changed,
        status,
        ..
    }) = structured
    {
        let verb = match status {
            WriteFileStatus::InProgress => "editing",
            WriteFileStatus::Completed => "edited",
            WriteFileStatus::Declined => "declined",
            WriteFileStatus::Failed => "failed",
        };
        return HistoryCell::edit(
            humanize_tool_label(tool_name),
            format!("{verb} {files_changed} files"),
            format_changed_paths_detail(changed_paths.iter().map(String::as_str)),
            match status {
                WriteFileStatus::Failed => HistoryTone::Error,
                WriteFileStatus::Declined => HistoryTone::Warning,
                _ => HistoryTone::Control,
            },
        );
    }
    if let Some(StructuredToolResult::ToolError { message, .. }) = structured {
        return HistoryCell::info(
            humanize_tool_label(tool_name),
            compact_inline(message, 100),
            HistoryTone::Error,
        );
    }

    if matches!(
        structured.map(classify_structured_result),
        Some(ToolOperation::Search | ToolOperation::Read)
    ) {
        let detail = content
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| compact_inline(line, 72))
            .unwrap_or_else(|| "completed".to_string());
        let mut aggregate = ExplorationAggregate::new(detail);
        if matches!(classify_tool_name(tool_name), ToolOperation::Search) {
            aggregate.searches = 1;
        } else {
            aggregate.metadata_reads = 1;
        }
        return HistoryCell::exploration(
            humanize_tool_label(tool_name),
            "completed".to_string(),
            aggregate,
            HistoryTone::Control,
        );
    }

    let first = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("tool completed");
    HistoryCell::info(
        humanize_tool_label(tool_name),
        compact_inline(first, 100),
        HistoryTone::Control,
    )
}

pub(super) fn render_file_change(
    tool_name: &str,
    path: &str,
    status: &WriteFileStatus,
    files_changed: usize,
    summary: &str,
) -> HistoryCell {
    let verb = match status {
        WriteFileStatus::InProgress => "editing",
        WriteFileStatus::Completed => "edited",
        WriteFileStatus::Declined => "declined",
        WriteFileStatus::Failed => "failed",
    };
    let detail = format_changed_paths_detail(path.split(',').map(str::trim)).or_else(|| {
        (matches!(status, WriteFileStatus::Failed | WriteFileStatus::Declined)
            && !summary.trim().is_empty())
        .then(|| compact_file_change_failure(summary))
    });
    HistoryCell::edit(
        humanize_tool_label(tool_name),
        format!("{verb} {files_changed} files"),
        detail,
        match status {
            WriteFileStatus::Failed => HistoryTone::Error,
            WriteFileStatus::Declined => HistoryTone::Warning,
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
    if classify_command_kind(command) != ToolUiKind::Exploration(ExplorationKind::WorkspaceCommand)
    {
        return None;
    }

    let command_preview = summarize_exploration_command(command);
    let mut aggregate = ExplorationAggregate::new(command_preview.clone());
    aggregate.inspect_commands = 1;

    Some(HistoryCell::exploration(
        exploration_label(ExplorationKind::WorkspaceCommand),
        command_preview,
        aggregate,
        HistoryTone::Control,
    ))
}

fn classify_command_kind(command: &str) -> ToolUiKind {
    if is_exploration_command(command) {
        ToolUiKind::Exploration(ExplorationKind::WorkspaceCommand)
    } else {
        ToolUiKind::Command
    }
}

fn exploration_label(kind: ExplorationKind) -> &'static str {
    match kind {
        ExplorationKind::WorkspaceCommand => "Explore workspace",
        ExplorationKind::ReadFile => "Read file",
        ExplorationKind::SearchWorkspace => "Search workspace",
        ExplorationKind::ReadDirectory => "Read directory",
        ExplorationKind::FileInfo => "File info",
    }
}

fn format_changed_paths_detail<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let paths = paths
        .into_iter()
        .filter(|path| !path.trim().is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return None;
    }

    let visible_paths = if paths.len() > 3 { 2 } else { 3 };
    let mut lines = paths
        .iter()
        .take(visible_paths)
        .map(|path| compact_path(path.trim(), 64))
        .collect::<Vec<_>>();
    if paths.len() > visible_paths {
        lines.push(format!("+{} more files", paths.len() - visible_paths));
    }
    Some(lines.join("\n"))
}

fn compact_file_change_failure(summary: &str) -> String {
    let text = summary.trim();
    if text.contains("Failed to find expected lines") {
        return "expected lines not found".to_string();
    }
    if text.contains("patch did not contain any editable file hunks") {
        return "invalid patch format".to_string();
    }
    if text.contains("refusing to add existing file") {
        return "file already exists".to_string();
    }
    if text.contains("refusing to update missing file") {
        return "file does not exist".to_string();
    }
    if text.contains("partial_changes=") {
        return compact_partial_change_failure(text);
    }
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("edit failed");
    let first = first
        .strip_prefix("Tool execution failed:")
        .unwrap_or(first)
        .trim();
    let first = first
        .strip_prefix("apply_patch failed:")
        .unwrap_or(first)
        .trim();
    compact_inline(first, 72)
}

fn compact_partial_change_failure(summary: &str) -> String {
    summary
        .lines()
        .find(|line| line.contains("partial_changes="))
        .map(|line| compact_inline(line.trim(), 72))
        .unwrap_or_else(|| "partial changes may have been written".to_string())
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

pub(crate) fn humanize_tool_label(tool_name: &str) -> String {
    match tool_name {
        "exec_command" | "tool" => "Run command".to_string(),
        "apply_patch" | "edit_file" => "Edit file".to_string(),
        WEB_SEARCH_TOOL_NAME => "Web search".to_string(),
        "read_file" => "Read file".to_string(),
        "read_directory" => "Read directory".to_string(),
        "search_workspace" => "Search workspace".to_string(),
        "tool_search" => "Search tools".to_string(),
        "get_metadata" => "File info".to_string(),
        "create_directory" => "Create directory".to_string(),
        "write_file" => "Write file".to_string(),
        "copy_path" => "Copy path".to_string(),
        "remove_path" => "Remove path".to_string(),
        other => other.replace('_', " "),
    }
}

fn compact_inline(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (index, ch) in input.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            return out;
        }
        out.push(if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        });
    }
    out
}

fn compact_path(path: &str, max_chars: usize) -> String {
    let path = path.replace('\\', "/");
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max_chars {
        return path;
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let tail_len = max_chars.saturating_sub(1);
    let tail: String = chars[chars.len().saturating_sub(tail_len)..]
        .iter()
        .collect();
    format!("…{tail}")
}

fn format_line_range(start_line: Option<usize>, end_line: Option<usize>) -> String {
    match (start_line, end_line) {
        (Some(start), Some(end)) if end >= start => format!(":{start}-{end}"),
        (Some(start), _) => format!(":{start}"),
        _ => String::new(),
    }
}
