use crate::tool::{CommandExecutionStatus, StructuredToolResult, WriteFileStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptItem {
    SystemMessage {
        id: String,
        text: String,
    },
    UserMessage {
        id: String,
        text: String,
    },
    AgentMessage {
        id: String,
        text: String,
    },
    CommandExecution {
        id: String,
        tool_name: String,
        command: String,
        current_directory: String,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        stdout: Option<String>,
        stderr: Option<String>,
        summary: String,
    },
    FileChange {
        id: String,
        tool_name: String,
        path: String,
        status: WriteFileStatus,
        bytes_written: usize,
        summary: String,
    },
    ToolResult {
        id: String,
        tool_name: String,
        content: String,
        summary: String,
        structured: Option<StructuredToolResult>,
    },
    Reasoning {
        id: String,
        title: String,
        text: String,
    },
}
