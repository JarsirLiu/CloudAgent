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
            tool_name,
            command,
            current_directory,
            status,
            exit_code,
            stderr,
            summary,
            ..
        } => HistoryCell::from_message(
            tool_name.clone(),
            summarize_command_execution(
                command,
                current_directory,
                status,
                *exit_code,
                stderr.as_deref().or(Some(summary.as_str())),
            ),
            HistoryTone::Control,
        ),
        TranscriptItem::FileChange {
            tool_name, summary, ..
        } => {
            HistoryCell::from_message(tool_name.clone(), summary.clone(), HistoryTone::Control)
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
    let kind = summarize_exec_command_kind(command);
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
    if let Some(StructuredToolResult::ListDirectory {
        path,
        shown_count,
        total_count,
        truncated,
        ..
    }) = structured
    {
        return format!(
            "listed {shown_count} of {total_count} entries{} @ {}",
            if *truncated { " truncated" } else { "" },
            compact_path(path, 48)
        );
    }
    if let Some(StructuredToolResult::ReadFiles {
        paths,
        file_count,
        truncated_count,
        total_chars,
        ..
    }) = structured
    {
        let target = paths
            .first()
            .map(|path| compact_path(path, 48))
            .unwrap_or_else(|| "workspace".to_string());
        if *file_count == 1 {
            return format!(
                "read {total_chars} chars{} @ {}",
                if *truncated_count > 0 { " truncated" } else { "" },
                target
            );
        }
        return format!(
            "read {file_count} files ({} truncated, {total_chars} chars)",
            truncated_count
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
        return match mode {
            agent_protocol::SearchWorkspaceMode::Files => {
                format!("found {file_count} files{truncation}{status_suffix}")
            }
            agent_protocol::SearchWorkspaceMode::Text => {
                format!(
                    "matched {match_count} hits in {file_count} files{truncation}{status_suffix}"
                )
            }
        };
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
            return format!("{verb} {files_changed} files @ {}", compact_path(path, 48));
        }
        return format!("{verb} {files_changed} files");
    }
    if let Some(StructuredToolResult::ToolError { message, .. }) = structured {
        return compact_inline(message, 100);
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

fn summarize_exec_command_kind(command: &str) -> &'static str {
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
