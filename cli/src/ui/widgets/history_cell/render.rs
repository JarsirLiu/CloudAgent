use super::{ExplorationAggregate, HistoryCell, HistoryFormat, HistoryTone};
use crate::app::conversation::exploration::{
    is_exploration_command, summarize_exploration_command,
};
use agent_protocol::{
    CommandExecutionStatus, StructuredToolResult, TranscriptItem, TurnItemKind, WriteFileStatus,
};

#[derive(Default)]
pub struct RenderContext;

pub fn render_history_entry(message: &TranscriptItem, context: &mut RenderContext) -> HistoryCell {
    match message {
        TranscriptItem::SystemMessage { .. } => HistoryCell::info("", "", HistoryTone::Meta),
        TranscriptItem::UserMessage { text, .. } => HistoryCell::user(text.clone()),
        TranscriptItem::AgentMessage { text, .. } => {
            let _ = context;
            HistoryCell::agent("", text.clone(), HistoryFormat::Markdown)
        }
        TranscriptItem::ToolResult {
            tool_name,
            content,
            structured,
            ..
        } => render_tool_result(tool_name, content, structured.as_ref()),
        TranscriptItem::CommandExecution {
            tool_name,
            command,
            current_directory,
            status,
            exit_code,
            stderr,
            summary,
            ..
        } => render_command_execution(
            tool_name,
            command,
            current_directory,
            status,
            *exit_code,
            stderr.as_deref().or(Some(summary.as_str())),
        ),
        TranscriptItem::FileChange {
            tool_name, summary, ..
        } => HistoryCell::edit(
            humanize_tool_label(tool_name),
            summary.clone(),
            None,
            HistoryTone::Control,
        ),
        TranscriptItem::Reasoning { text, .. } => HistoryCell::reasoning("Reasoning", text.clone()),
    }
}

pub fn render_active_item_placeholder(kind: TurnItemKind, title: &str) -> HistoryCell {
    match kind {
        TurnItemKind::AssistantMessage => {
            HistoryCell::agent("", "responding".to_string(), HistoryFormat::Markdown)
        }
        TurnItemKind::Reasoning => HistoryCell::reasoning("Reasoning", "thinking".to_string()),
        TurnItemKind::CommandExecution if is_exploration_command(title) => {
            let summary = summarize_exploration_command(title);
            let mut aggregate = ExplorationAggregate::new(summary.clone());
            aggregate.inspect_commands = 1;
            HistoryCell::exploration(
                "Exploring workspace",
                summary,
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
        _ => HistoryCell::info(
            title.to_string(),
            "running".to_string(),
            HistoryTone::Control,
        ),
    }
}

fn render_command_execution(
    tool_name: &str,
    command: &str,
    current_directory: &str,
    status: &CommandExecutionStatus,
    exit_code: Option<i32>,
    detail: Option<&str>,
) -> HistoryCell {
    if let Some(exploration) =
        render_exploration_command(tool_name, command, current_directory, status)
    {
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

fn render_exploration_command(
    tool_name: &str,
    command: &str,
    current_directory: &str,
    status: &CommandExecutionStatus,
) -> Option<HistoryCell> {
    if !is_exploration_command(command) {
        return None;
    }

    let summary = summarize_exploration_command(command);
    let cwd = compact_path(current_directory, 42);
    let mut aggregate = ExplorationAggregate::new(summary.clone());
    aggregate.inspect_commands = 1;

    let cell = HistoryCell::exploration(
        "Explored workspace",
        "ran 1 inspect command".to_string(),
        aggregate.clone(),
        HistoryTone::Control,
    );
    let _ = (tool_name, cwd, status);
    Some(cell)
}

fn render_tool_result(
    tool_name: &str,
    content: &str,
    structured: Option<&StructuredToolResult>,
) -> HistoryCell {
    if let Some(StructuredToolResult::CommandExecution {
        command,
        current_directory,
        status,
        exit_code,
        stderr,
        ..
    }) = structured
    {
        return render_command_execution(
            tool_name,
            command,
            current_directory,
            status,
            *exit_code,
            stderr.as_deref(),
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
        let cell = HistoryCell::edit(
            humanize_tool_label(tool_name),
            "read 1 file".to_string(),
            Some(detail),
            HistoryTone::Control,
        );
        return cell;
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
            agent_protocol::SearchWorkspaceStatus::Active => "",
            agent_protocol::SearchWorkspaceStatus::Closed => " closed",
            agent_protocol::SearchWorkspaceStatus::NotFound => " missing",
        };
        let truncation = if *truncated { " truncated" } else { "" };
        let summary = match mode {
            agent_protocol::SearchWorkspaceMode::Files => {
                format!("found {file_count} files{truncation}{status_suffix}")
            }
            agent_protocol::SearchWorkspaceMode::Text => {
                format!(
                    "matched {match_count} hits in {file_count} files{truncation}{status_suffix}"
                )
            }
        };
        let detail = match mode {
            agent_protocol::SearchWorkspaceMode::Files => {
                format!("file search `{}`", compact_inline(query, 48))
            }
            agent_protocol::SearchWorkspaceMode::Text => {
                format!("text search `{}`", compact_inline(query, 48))
            }
        };
        let cell = HistoryCell::edit(
            humanize_tool_label(tool_name),
            summary,
            Some(detail),
            HistoryTone::Control,
        );
        return cell;
    }
    if let Some(StructuredToolResult::ReadDirectory {
        path,
        entry_count,
        truncated,
        ..
    }) = structured
    {
        let detail = format!(
            "{} • {} entries{}",
            compact_path(path, 56),
            entry_count,
            if *truncated { " truncated" } else { "" }
        );
        let cell = HistoryCell::edit(
            humanize_tool_label(tool_name),
            "listed 1 directory".to_string(),
            Some(detail),
            HistoryTone::Control,
        );
        return cell;
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
            let detail = format!(
                "metadata {} • {kind} ({size} bytes)",
                compact_path(path, 56)
            );
            let cell = HistoryCell::edit(
                humanize_tool_label(tool_name),
                "checked 1 path".to_string(),
                Some(detail),
                HistoryTone::Control,
            );
            return cell;
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
        if let Some(path) = changed_paths.first() {
            return HistoryCell::edit(
                humanize_tool_label(tool_name),
                format!("{verb} {files_changed} files"),
                Some(compact_path(path, 48)),
                HistoryTone::Control,
            );
        }
        return HistoryCell::edit(
            humanize_tool_label(tool_name),
            format!("{verb} {files_changed} files"),
            None,
            HistoryTone::Control,
        );
    }
    if let Some(StructuredToolResult::ToolError { message, .. }) = structured {
        return HistoryCell::info(
            humanize_tool_label(tool_name),
            compact_inline(message, 100),
            HistoryTone::Error,
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
        "read_file" => "Read file".to_string(),
        "read_directory" => "Read directory".to_string(),
        "search_workspace" => "Search workspace".to_string(),
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
