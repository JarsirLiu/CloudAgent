mod shared;
mod tools {
    pub mod command;
    pub mod edit;
    pub mod fs;
    pub mod repo;
}

use crate::impls::{
    code_editing::{ApplyPatchTool, WriteFileToolV2},
    command::ShellCommandToolV2,
    fs::{GetMetadataTool, ReadDirectoryToolV2},
    repo::{FindFilesTool, ReadFileToolV2, ReadFilesTool, SearchTextTool},
};
use crate::selection::{TaskKind, ToolMode, ToolSelector};
use crate::spec::ToolDescriptor;
use agent_core::{ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, ToolSpec};
use anyhow::{Result, bail};
use async_trait::async_trait;

use shared::{LocalTool, register, structured_failure_result};
use std::collections::BTreeMap;
use std::sync::Arc;
use tools::command::ShellCommandTool;
use tools::edit::WriteFileTool;
use tools::fs::{GetMetadataLocalTool, ReadDirectoryTool};
use tools::repo::{FindFilesLocalTool, ReadFileTool, ReadFilesLocalTool, SearchTextLocalTool};

#[derive(Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn LocalTool>>,
    descriptors: Vec<ToolDescriptor>,
    selector: ToolSelector,
}

impl ToolRegistry {
    pub fn new(max_read_chars: usize) -> Self {
        let descriptors = vec![
            SearchTextTool::descriptor(),
            FindFilesTool::descriptor(),
            ReadFileToolV2::descriptor(max_read_chars),
            ReadFilesTool::descriptor(max_read_chars),
            ApplyPatchTool::descriptor(),
            WriteFileToolV2::descriptor(),
            ShellCommandToolV2::descriptor(),
            GetMetadataTool::descriptor(),
            ReadDirectoryToolV2::descriptor(),
        ];

        let mut tools: BTreeMap<String, Arc<dyn LocalTool>> = BTreeMap::new();
        register(&mut tools, ShellCommandTool);
        register_alias(&mut tools, "command/exec", "shell_command");
        register(&mut tools, SearchTextLocalTool);
        register(&mut tools, FindFilesLocalTool);
        register(&mut tools, ReadFilesLocalTool { max_read_chars });
        register(&mut tools, GetMetadataLocalTool);
        register_alias(&mut tools, "fs/getMetadata", "get_metadata");
        register(&mut tools, ReadDirectoryTool);
        register_alias(&mut tools, "fs/readDirectory", "read_directory");
        register(&mut tools, ReadFileTool { max_read_chars });
        register_alias(&mut tools, "fs/readFile", "read_file");
        register(&mut tools, WriteFileTool);
        register_alias(&mut tools, "fs/writeFile", "write_file");

        Self {
            tools,
            descriptors,
            selector: ToolSelector::new(),
        }
    }

    pub fn specs_for_mode(&self, mode: ToolMode, task_kind: TaskKind) -> Vec<ToolSpec> {
        self.selector
            .select(&mode, &task_kind, &self.descriptors)
            .into_iter()
            .map(|descriptor| descriptor.spec.clone())
            .collect()
    }
}

fn register_alias(
    tools: &mut BTreeMap<String, Arc<dyn LocalTool>>,
    alias: &str,
    target: &str,
) {
    if let Some(tool) = tools.get(target).cloned() {
        tools.insert(alias.to_string(), tool);
    }
}

#[async_trait]
impl ToolExecutor for ToolRegistry {
    fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    fn specs_for_context(&self, mode: &str, task_kind: &str) -> Vec<ToolSpec> {
        let parsed_mode = match mode {
            "explore" => ToolMode::Explore,
            "edit" => ToolMode::Edit,
            "verify" => ToolMode::Verify,
            _ => ToolMode::Full,
        };
        let parsed_task = match task_kind {
            "repository_analysis" => TaskKind::RepositoryAnalysis,
            "code_edit" => TaskKind::CodeEdit,
            "verification" => TaskKind::Verification,
            "workspace_file_operation" => TaskKind::WorkspaceFileOperation,
            _ => TaskKind::General,
        };
        self.specs_for_mode(parsed_mode, parsed_task)
    }

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let call_name = call.name.clone();
        let call_args = call.arguments.clone();
        let Some(tool) = self.tools.get(&call.name) else {
            bail!("tool `{}` is not registered", call.name);
        };

        match tool.invoke(call.arguments, ctx).await {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: output.content,
                summary: output.summary,
                is_error: false,
                structured: output.structured,
            }),
            Err(err) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: format!("Tool execution failed: {err:#}"),
                summary: "tool execution failed".to_string(),
                is_error: true,
                structured: structured_failure_result(&call_name, &call_args),
            }),
        }
    }
}
