use agent_core::{ToolCall, ToolResult};
use agent_protocol::{
    CommandExecutionStatus, StructuredToolResult, TranscriptItem, WriteFileStatus,
};

pub(crate) fn transcript_item_from_tool_result(
    item_id: &str,
    tool_name: &str,
    result: &ToolResult,
) -> TranscriptItem {
    match &result.structured {
        Some(StructuredToolResult::CommandExecution {
            command,
            current_directory,
            status,
            exit_code,
            stdout,
            stderr,
            aggregated_output,
            duration_ms,
            ..
        }) => TranscriptItem::CommandExecution {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            command: command.clone(),
            current_directory: current_directory.clone(),
            status: status.clone(),
            exit_code: *exit_code,
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            aggregated_output: aggregated_output.clone(),
            duration_ms: *duration_ms,
            summary: result.content.clone(),
        },
        Some(StructuredToolResult::EditFile {
            changed_paths,
            files_changed,
            status,
        }) => TranscriptItem::FileChange {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            path: changed_paths.join(", "),
            status: status.clone(),
            bytes_written: *files_changed,
            summary: result.content.clone(),
        },
        _ => TranscriptItem::ToolResult {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            content: result.content.clone(),
            summary: result.content.clone(),
            structured: result.structured.clone(),
        },
    }
}

pub(crate) fn denied_transcript_item(
    item_id: &str,
    tool_name: &str,
    arguments: &serde_json::Value,
    reason: &str,
) -> TranscriptItem {
    match denied_tool_result(tool_name, arguments, reason.to_string()) {
        Some(StructuredToolResult::CommandExecution {
            command,
            current_directory,
            status,
            exit_code,
            stdout,
            stderr,
            aggregated_output,
            duration_ms,
            ..
        }) => TranscriptItem::CommandExecution {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            command,
            current_directory,
            status,
            exit_code,
            stdout,
            stderr,
            aggregated_output,
            duration_ms,
            summary: reason.to_string(),
        },
        Some(StructuredToolResult::EditFile {
            changed_paths,
            files_changed,
            status,
        }) => TranscriptItem::FileChange {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            path: changed_paths.join(", "),
            status,
            bytes_written: files_changed,
            summary: reason.to_string(),
        },
        structured => TranscriptItem::ToolResult {
            id: item_id.to_string(),
            tool_name: tool_name.to_string(),
            content: reason.to_string(),
            summary: reason.to_string(),
            structured,
        },
    }
}

pub(crate) fn tool_item_title(call: &ToolCall) -> String {
    if call.name == "exec_command"
        && let Some(command) = call.arguments.get("command").and_then(|value| value.as_str())
        && !command.trim().is_empty()
    {
        return command.trim().to_string();
    }
    if call.name == "exec_command"
        && let Some(session_id) = call.arguments.get("session_id").and_then(|value| value.as_str())
        && !session_id.trim().is_empty()
    {
        return format!("session {}", session_id.trim());
    }
    call.name.clone()
}

pub(crate) fn denied_tool_result(
    tool_name: &str,
    arguments: &serde_json::Value,
    reason: String,
) -> Option<StructuredToolResult> {
    match tool_name {
        "exec_command" => {
            let command = arguments
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let current_directory = arguments
                .get("workdir")
                .and_then(|value| value.as_str())
                .unwrap_or(".")
                .to_string();
            Some(StructuredToolResult::CommandExecution {
                command,
                current_directory,
                session_id: None,
                status: CommandExecutionStatus::Declined,
                exit_code: None,
                success: Some(false),
                stdout: None,
                stderr: Some(reason),
                aggregated_output: None,
                duration_ms: None,
            })
        }
        "apply_patch" => Some(StructuredToolResult::EditFile {
            changed_paths: Vec::new(),
            files_changed: 0,
            status: WriteFileStatus::Declined,
        }),
        _ => Some(StructuredToolResult::ToolError {
            tool_name: tool_name.to_string(),
            message: reason,
        }),
    }
}

pub(crate) fn default_rejection_message(tool_name: &str) -> String {
    match tool_name {
        "exec_command" => {
            "exec command rejected by user: the user denied this approval request; do not describe this as a system safety restriction".to_string()
        }
        "apply_patch" => {
            "edit rejected by user: the user denied this approval request; do not describe this as a system safety restriction".to_string()
        }
        _ => {
            "tool call rejected by user: the user denied this approval request; do not describe this as a system safety restriction".to_string()
        }
    }
}

pub(crate) fn repeated_rejection_message(tool_name: &str) -> String {
    format!(
        "{}; same tool request was already denied in this turn",
        default_rejection_message(tool_name)
    )
}

pub(crate) fn tool_request_key(call: &ToolCall) -> String {
    format!("{}:{}", call.name, canonical_json(&call.arguments))
}

pub(crate) fn missing_tool_result(call: &ToolCall) -> ToolResult {
    let message = format!("Tool `{}` is not registered.", call.name);
    ToolResult {
        tool_call_id: call.id.clone(),
        name: call.name.clone(),
        content: message,
        is_error: true,
        structured: Some(StructuredToolResult::ToolError {
            tool_name: call.name.clone(),
            message: format!("Tool `{}` is not registered.", call.name),
        }),
    }
}

fn canonical_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}
