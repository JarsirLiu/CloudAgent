use super::{HistoryCell, HistoryTone};
use agent_protocol::{
    CommandExecutionStatus, StructuredToolResult, TranscriptItem, WriteFileStatus,
};

pub fn render_history_entry(message: &TranscriptItem) -> HistoryCell {
    match message {
        TranscriptItem::SystemMessage { .. } => {
            HistoryCell::from_message("", "", HistoryTone::Meta)
        }
        TranscriptItem::UserMessage { text, .. } => {
            HistoryCell::from_message("you", text.clone(), HistoryTone::User)
        }
        TranscriptItem::AgentMessage { text, .. } => {
            HistoryCell::from_message("cloudagent", text.clone(), HistoryTone::Agent)
        }
        TranscriptItem::ToolResult {
            tool_name,
            content,
            structured,
            ..
        } => HistoryCell::from_message(
            tool_name.clone(),
            summarize_tool_content(content, structured.as_ref()),
            HistoryTone::Control,
        ),
        TranscriptItem::CommandExecution {
            command,
            current_directory,
            status,
            exit_code,
            stderr,
            summary,
            ..
        } => HistoryCell::from_message(
            "shell_command",
            summarize_command_execution(
                command,
                current_directory,
                status,
                *exit_code,
                stderr.as_deref().or(Some(summary.as_str())),
            ),
            HistoryTone::Control,
        ),
        TranscriptItem::FileChange { summary, .. } => {
            HistoryCell::from_message("file_change", summary.clone(), HistoryTone::Control)
        }
        TranscriptItem::Reasoning { text, .. } => {
            HistoryCell::from_message("reasoning", text.clone(), HistoryTone::Reasoning)
        }
    }
}

fn summarize_command_execution(
    command: &str,
    current_directory: &str,
    status: &CommandExecutionStatus,
    exit_code: Option<i32>,
    detail: Option<&str>,
) -> String {
    let kind = summarize_shell_command_kind(command);
    let command = compact_inline(command, 56);
    let cwd = compact_path(current_directory, 36);
    match status {
        CommandExecutionStatus::InProgress => format!("{kind} `{command}` @ {cwd}"),
        CommandExecutionStatus::Completed => {
            format!("{kind} `{command}`{} @ {cwd}", exit_suffix(exit_code))
        }
        CommandExecutionStatus::Declined => format!("{kind} `{command}` @ {cwd}"),
        CommandExecutionStatus::Failed => {
            let reason = detail
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| compact_inline(value, 90))
                .unwrap_or_else(|| "command failed".to_string());
            format!(
                "{kind} `{command}`{} @ {cwd}: {reason}",
                exit_suffix(exit_code)
            )
        }
    }
}

fn summarize_tool_content(content: &str, structured: Option<&StructuredToolResult>) -> String {
    if let Some(StructuredToolResult::CommandExecution {
        command,
        current_directory,
        status,
        exit_code,
        stderr,
        ..
    }) = structured
    {
        return summarize_command_execution(
            command,
            current_directory,
            status,
            *exit_code,
            stderr.as_deref(),
        );
    }
    if let Some(StructuredToolResult::ListDirectory { path, entry_count }) = structured {
        return format!("listed {entry_count} entries @ {}", compact_path(path, 48));
    }
    if let Some(StructuredToolResult::ReadFile {
        path,
        truncated,
        char_count,
    }) = structured
    {
        let truncation = if *truncated { " truncated" } else { "" };
        return format!(
            "read {char_count} chars{truncation} @ {}",
            compact_path(path, 48)
        );
    }
    if let Some(StructuredToolResult::WriteFile {
        path,
        bytes_written,
        status,
    }) = structured
    {
        let verb = match status {
            WriteFileStatus::InProgress => "writing",
            WriteFileStatus::Completed => "wrote",
            WriteFileStatus::Declined => "declined",
            WriteFileStatus::Failed => "failed",
        };
        return format!("{verb} {bytes_written} bytes @ {}", compact_path(path, 48));
    }
    if let Some(StructuredToolResult::ReadFiles { file_count }) = structured {
        return format!("read {file_count} files");
    }
    if let Some(StructuredToolResult::FindFiles { file_count }) = structured {
        return format!("found {file_count} files");
    }
    if let Some(StructuredToolResult::SearchText {
        match_count,
        file_count,
        truncated,
    }) = structured
    {
        let truncation = if *truncated { " truncated" } else { "" };
        return format!("matched {match_count} hits in {file_count} files{truncation}");
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
            return format!("metadata {kind} {} ({size} bytes)", compact_path(path, 48));
        }
        return format!("metadata missing {}", compact_path(path, 48));
    }
    if let Some(StructuredToolResult::EditFile {
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
        return format!("{verb} {files_changed} files");
    }

    let first = content
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("tool completed");
    compact_inline(first, 100)
}

fn exit_suffix(exit_code: Option<i32>) -> String {
    exit_code
        .map(|code| format!(" (exit {code})"))
        .unwrap_or_default()
}

fn summarize_shell_command_kind(command: &str) -> &'static str {
    let normalized = command.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return "command";
    }
    if normalized.starts_with("rg ")
        || normalized.starts_with("grep ")
        || normalized.starts_with("findstr ")
        || normalized.starts_with("select-string ")
        || normalized.starts_with("git grep ")
    {
        return "search";
    }
    if normalized.starts_with("git ls-files") || normalized.starts_with("fd ") {
        return "files";
    }
    if normalized.starts_with("git log")
        || normalized.starts_with("git status")
        || normalized.starts_with("git diff")
        || normalized.starts_with("git show")
        || normalized.starts_with("pwd")
        || normalized.starts_with("ls ")
        || normalized.starts_with("dir ")
        || normalized.starts_with("cat ")
        || normalized.starts_with("type ")
    {
        return "inspect";
    }
    "command"
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
