pub(crate) mod shared;

use crate::impls::{
    command::ShellCommandTool as ShellCommandDescriptorTool,
    fs::{
        EditFileTool, GetMetadataTool, ReadDirectoryTool as ReadDirectoryDescriptorTool,
        WriteFileTool as WriteFileDescriptorTool,
    },
    repo::{FindFilesTool, ReadFileTool as ReadFileDescriptorTool, ReadFilesTool, SearchTextTool},
};
use crate::selection::{TaskKind, ToolMode, ToolSelector};
use crate::spec::ToolDescriptor;
use agent_core::{ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, ToolSpec};
use anyhow::{Result, bail};
use async_trait::async_trait;

use shared::{LocalTool, register, structured_failure_result};
use std::collections::BTreeMap;
use std::sync::Arc;
use crate::impls::command::ShellCommandLocalTool;
use crate::impls::fs::{
    EditFileLocalTool, GetMetadataLocalTool, ReadDirectoryLocalTool, WriteFileLocalTool,
};
use crate::impls::repo::{FindFilesLocalTool, ReadFileLocalTool, ReadFilesLocalTool, SearchTextLocalTool};

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
            ReadFileDescriptorTool::descriptor(max_read_chars),
            ReadFilesTool::descriptor(max_read_chars),
            EditFileTool::descriptor(),
            WriteFileDescriptorTool::descriptor(),
            ShellCommandDescriptorTool::descriptor(),
            GetMetadataTool::descriptor(),
            ReadDirectoryDescriptorTool::descriptor(),
        ];

        let mut tools: BTreeMap<String, Arc<dyn LocalTool>> = BTreeMap::new();
        register(&mut tools, ShellCommandLocalTool);
        register(&mut tools, SearchTextLocalTool);
        register(&mut tools, FindFilesLocalTool);
        register(&mut tools, ReadFilesLocalTool { max_read_chars });
        register(&mut tools, GetMetadataLocalTool);
        register(&mut tools, ReadDirectoryLocalTool);
        register(&mut tools, ReadFileLocalTool { max_read_chars });
        register(&mut tools, WriteFileLocalTool);
        register(&mut tools, EditFileLocalTool);

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
                is_error: false,
                structured: output.structured,
            }),
            Err(err) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: format!("Tool execution failed: {err:#}"),
                is_error: true,
                structured: structured_failure_result(&call_name, &call_args),
            }),
        }
    }
}
