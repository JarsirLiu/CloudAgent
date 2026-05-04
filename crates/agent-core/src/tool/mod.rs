use crate::context::ToolExecutionContext;
use crate::conversation::TranscriptItem;
use crate::turn::{TurnItemDeltaKind, TurnItemKind};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

mod batch;
mod execution;

pub(crate) use batch::run_host_tool_batch;
pub use execution::{
    ParallelToolInvocation, ParallelToolResult, execute_tool_call_streaming,
    run_parallel_tool_invocations,
};

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

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct ApprovalGrantKey {
    pub kind: String,
    pub value: Value,
}

impl ApprovalGrantKey {
    pub fn new(kind: impl Into<String>, value: Value) -> Self {
        Self {
            kind: kind.into(),
            value,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    BuiltIn,
    Mcp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolIdentity {
    pub source: ToolSource,
    pub namespace: Option<String>,
    pub wire_name: String,
}

impl ToolIdentity {
    pub fn built_in(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            source: ToolSource::BuiltIn,
            namespace: None,
            wire_name: name,
        }
    }

    pub fn mcp(
        namespace: impl Into<String>,
        _tool: impl Into<String>,
        wire_name: impl Into<String>,
    ) -> Self {
        Self {
            source: ToolSource::Mcp,
            namespace: Some(namespace.into()),
            wire_name: wire_name.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub identity: ToolIdentity,
    pub description: String,
    pub parameters: Value,
    pub mutating: bool,
    pub requires_approval: bool,
    pub item_kind: TurnItemKind,
    pub delta_kind: TurnItemDeltaKind,
    pub approval_reason: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ResolvedToolSet {
    pub specs: Vec<ToolSpec>,
    parallel_tool_names: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolBatchExecutionStrategy {
    Sequential,
    Parallel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRequirement {
    pub requires_approval: bool,
    pub reason: Option<String>,
}

impl ApprovalRequirement {
    pub fn not_required() -> Self {
        Self {
            requires_approval: false,
            reason: None,
        }
    }

    pub fn required(reason: impl Into<String>) -> Self {
        Self {
            requires_approval: true,
            reason: Some(reason.into()),
        }
    }
}

impl ResolvedToolSet {
    pub fn new(specs: Vec<ToolSpec>) -> Self {
        Self {
            specs,
            parallel_tool_names: BTreeSet::new(),
        }
    }

    pub fn supports_parallel_tool(&self, tool_name: &str) -> bool {
        self.parallel_tool_names.contains(tool_name)
    }

    pub fn mark_parallel_tool(&mut self, tool_name: String) {
        self.parallel_tool_names.insert(tool_name);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub identity: ToolIdentity,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    RepositoryAnalysis,
    CodeEdit,
    Verification,
    WorkspaceFileOperation,
    General,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolMode {
    Explore,
    Edit,
    Verify,
    Full,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolSurface {
    pub mode: ToolMode,
    pub task_kind: TaskKind,
}

impl ToolSurface {
    pub fn new(mode: ToolMode, task_kind: TaskKind) -> Self {
        Self { mode, task_kind }
    }

    pub fn regular_turn() -> Self {
        Self::new(ToolMode::Full, TaskKind::General)
    }
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
pub struct SearchWorkspaceHit {
    pub path: String,
    pub line: Option<usize>,
    pub preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadFileEntry {
    pub path: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub truncated: bool,
    pub char_count: usize,
    pub status: ReadFileStatus,
    #[serde(default)]
    pub version_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpCallResult {
    pub content: Value,
    pub structured_content: Option<Value>,
    pub is_error: bool,
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
        next_offset: Option<usize>,
        hits: Vec<SearchWorkspaceHit>,
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
        failed_count: usize,
        truncated_count: usize,
        total_chars: usize,
        reads: Vec<ReadFileEntry>,
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
        #[serde(default)]
        version_token: Option<String>,
    },
    McpToolCall {
        server: String,
        tool: String,
        result: McpCallResult,
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
pub enum ReadFileStatus {
    Ok,
    Binary,
    TooLarge,
    UnsupportedEncoding,
    Failed,
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
    fn specs_for_surface(&self, _surface: &ToolSurface) -> Vec<ToolSpec> {
        self.specs()
    }

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult>;
}

pub trait ToolBackend: ToolExecutor {
    type PermissionProfile: Send + Sync;
    type ApprovalPolicy: Send + Sync;

    fn resolve_surface(
        &self,
        tool_surface: &ToolSurface,
        permission_profile: &Self::PermissionProfile,
    ) -> ResolvedToolSet;

    fn batch_execution_strategy(&self, calls: &[ToolCall]) -> ToolBatchExecutionStrategy;

    fn approval_requirement_for_call(
        &self,
        spec: &ToolSpec,
        call: &ToolCall,
        workspace_root: &Path,
        permission_profile: &Self::PermissionProfile,
        approval_policy: &Self::ApprovalPolicy,
    ) -> ApprovalRequirement;

    fn approval_grant_key_for_call(
        &self,
        spec: &ToolSpec,
        call: &ToolCall,
        workspace_root: &Path,
        permission_profile: &Self::PermissionProfile,
        approval_policy: &Self::ApprovalPolicy,
    ) -> Option<ApprovalGrantKey>;

    fn tool_item_title(&self, call: &ToolCall) -> String;

    fn transcript_item_from_result(
        &self,
        item_id: &str,
        call: &ToolCall,
        result: &ToolResult,
    ) -> TranscriptItem;

    fn denied_transcript_item(
        &self,
        item_id: &str,
        call: &ToolCall,
        reason: &str,
    ) -> TranscriptItem;

    fn default_rejection_message(&self, tool_name: &str) -> String;

    fn repeated_rejection_message(&self, tool_name: &str) -> String;

    fn denied_structured_result(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        reason: String,
    ) -> Option<StructuredToolResult>;

    fn tool_request_key(&self, call: &ToolCall) -> String;

    fn missing_tool_result(&self, call: &ToolCall) -> ToolResult;
}

pub fn summarize_arguments(arguments: &Value) -> String {
    let rendered =
        serde_json::to_string(arguments).unwrap_or_else(|_| "<invalid-json>".to_string());
    if rendered.chars().count() > 240 {
        let truncated = rendered.chars().take(240).collect::<String>();
        format!("{truncated}...")
    } else {
        rendered
    }
}
