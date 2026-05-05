use super::{HistoryCell, HistoryFormat, HistoryKind, HistoryTone};
use agent_protocol::{
    CommandExecutionStatus, StructuredToolResult, TranscriptItem, WriteFileStatus,
};

#[derive(Default)]
pub struct RenderContext {
    response_index: usize,
}

impl RenderContext {
    fn next_response_label(&mut self) -> String {
        self.response_index = self.response_index.saturating_add(1);
        format!("Response {}", self.response_index)
    }

    fn current_reasoning_label(&self) -> String {
        if self.response_index == 0 {
            "Reasoning".to_string()
        } else {
            format!("Reasoning {}", self.response_index)
        }
    }
}

pub fn render_history_entry(message: &TranscriptItem, context: &mut RenderContext) -> HistoryCell {
    match message {
        TranscriptItem::SystemMessage { .. } => {
            HistoryCell::from_message("", "", HistoryTone::Meta)
        }
        TranscriptItem::UserMessage { text, .. } => {
            HistoryCell::from_message("you", text.clone(), HistoryTone::User)
        }
        TranscriptItem::AgentMessage { text, .. } => {
            HistoryCell::with_parts(
                context.next_response_label(),
                text.clone(),
                HistoryTone::Agent,
                HistoryKind::Message,
                HistoryFormat::Markdown,
                None,
            )
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
        } => HistoryCell::with_parts(
            humanize_tool_label(tool_name),
            summary.clone(),
            HistoryTone::Control,
            HistoryKind::Tool,
            HistoryFormat::PlainText,
            None,
        ),
        TranscriptItem::Reasoning { text, .. } => HistoryCell::with_parts(
            context.current_reasoning_label(),
            text.clone(),
            HistoryTone::Reasoning,
            HistoryKind::Reasoning,
            HistoryFormat::PlainText,
            None,
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

    HistoryCell::with_parts(
        humanize_tool_label(tool_name),
        summary,
        match status {
            CommandExecutionStatus::Failed => HistoryTone::Error,
            CommandExecutionStatus::Declined => HistoryTone::Warning,
            _ => HistoryTone::Control,
        },
        HistoryKind::Command,
        HistoryFormat::PlainText,
        Some(format!("{state} @ {cwd}")),
    )
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
        return HistoryCell::with_parts(
            humanize_tool_label(tool_name),
            format!(
                "read {total_chars} chars{}",
                if read.truncated { " truncated" } else { "" }
            ),
            HistoryTone::Control,
            HistoryKind::Tool,
            HistoryFormat::PlainText,
            Some(compact_path(path, 48)),
        );
    }
    if let Some(StructuredToolResult::SearchWorkspace {
        mode,
        status,
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
                format!("matched {match_count} hits in {file_count} files{truncation}{status_suffix}")
            }
        };
        return HistoryCell::with_parts(
            humanize_tool_label(tool_name),
            summary,
            HistoryTone::Control,
            HistoryKind::Tool,
            HistoryFormat::PlainText,
            None,
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
            return HistoryCell::with_parts(
                humanize_tool_label(tool_name),
                format!("metadata {kind} ({size} bytes)"),
                HistoryTone::Control,
                HistoryKind::Tool,
                HistoryFormat::PlainText,
                Some(compact_path(path, 48)),
            );
        }
        return HistoryCell::with_parts(
            humanize_tool_label(tool_name),
            "metadata missing".to_string(),
            HistoryTone::Warning,
            HistoryKind::Tool,
            HistoryFormat::PlainText,
            Some(compact_path(path, 48)),
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
            return HistoryCell::with_parts(
                humanize_tool_label(tool_name),
                format!("{verb} {files_changed} files"),
                HistoryTone::Control,
                HistoryKind::Tool,
                HistoryFormat::PlainText,
                Some(compact_path(path, 48)),
            );
        }
        return HistoryCell::with_parts(
            humanize_tool_label(tool_name),
            format!("{verb} {files_changed} files"),
            HistoryTone::Control,
            HistoryKind::Tool,
            HistoryFormat::PlainText,
            None,
        );
    }
    if let Some(StructuredToolResult::ToolError { message, .. }) = structured {
        return HistoryCell::with_parts(
            humanize_tool_label(tool_name),
            compact_inline(message, 100),
            HistoryTone::Error,
            HistoryKind::Tool,
            HistoryFormat::PlainText,
            None,
        );
    }

    let first = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("tool completed");
    HistoryCell::with_parts(
        humanize_tool_label(tool_name),
        compact_inline(first, 100),
        HistoryTone::Control,
        HistoryKind::Tool,
        HistoryFormat::PlainText,
        None,
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

fn humanize_tool_label(tool_name: &str) -> String {
    match tool_name {
        "exec_command" | "tool" => "Run command".to_string(),
        "apply_patch" | "edit_file" => "Edit file".to_string(),
        "read_file" => "Read file".to_string(),
        "search_workspace" => "Search workspace".to_string(),
        "get_metadata" => "File info".to_string(),
        "write_file" => "Write file".to_string(),
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
