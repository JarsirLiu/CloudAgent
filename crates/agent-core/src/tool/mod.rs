use crate::context::ToolExecutionContext;
use crate::turn::{TurnItemDeltaKind, TurnItemKind};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct ToolOutputDelta {
    pub stream: ToolOutputStream,
    pub chunk: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolOutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub mutating: bool,
    pub requires_approval: bool,
    pub item_kind: TurnItemKind,
    pub delta_kind: TurnItemDeltaKind,
    pub approval_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
    pub structured: Option<StructuredToolResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StructuredToolResult {
    ToolError {
        tool_name: String,
        message: String,
    },
    CommandExecution {
        command: String,
        current_directory: String,
        session_id: Option<String>,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        success: Option<bool>,
        stdout: Option<String>,
        stderr: Option<String>,
        aggregated_output: Option<String>,
        duration_ms: Option<u64>,
    },
    SearchWorkspace {
        session_id: String,
        operation: SearchWorkspaceOperation,
        mode: SearchWorkspaceMode,
        status: SearchWorkspaceStatus,
        query: String,
        path_scope: Option<String>,
        case_sensitive: bool,
        context_lines: usize,
        max_results: usize,
        offset: usize,
        file_count: usize,
        match_count: usize,
        truncated: bool,
    },
    ListDirectory {
        path: String,
        recursive: bool,
        offset: usize,
        shown_count: usize,
        total_count: usize,
        truncated: bool,
    },
    ReadFiles {
        paths: Vec<String>,
        start_line: Option<usize>,
        max_lines: Option<usize>,
        file_count: usize,
        truncated_count: usize,
        total_chars: usize,
    },
    GetMetadata {
        path: String,
        exists: bool,
        is_file: bool,
        is_dir: bool,
        size: u64,
        readonly: bool,
    },
    EditFile {
        changed_paths: Vec<String>,
        files_changed: usize,
        status: WriteFileStatus,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SearchWorkspaceOperation {
    Search,
    Close,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SearchWorkspaceMode {
    Files,
    Text,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SearchWorkspaceStatus {
    Active,
    Closed,
    NotFound,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CommandExecutionStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WriteFileStatus {
    InProgress,
    Completed,
    Declined,
    Failed,
}

#[derive(Clone, Debug)]
pub struct ToolEvent {
    pub name: String,
    pub summary: String,
    pub is_error: bool,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn specs(&self) -> Vec<ToolSpec>;
    fn specs_for_context(&self, _mode: &str, _task_kind: &str) -> Vec<ToolSpec> {
        self.specs()
    }

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult>;
}
