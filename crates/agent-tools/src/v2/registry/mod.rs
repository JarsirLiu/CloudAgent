use super::impls::{
    code_editing::{ApplyPatchTool, WriteFileToolV2},
    command_execution::ShellCommandToolV2,
    repository_exploration::{FindFilesTool, ReadFileToolV2, ReadFilesTool, SearchTextTool},
    workspace_file_ops::{GetMetadataTool, ReadDirectoryToolV2},
};
use super::selection::{TaskKind, ToolMode, ToolSelector};
use super::spec::ToolDescriptor;
use agent_core::ToolSpec;

#[derive(Clone, Debug)]
pub struct ToolRegistryV2 {
    descriptors: Vec<ToolDescriptor>,
    selector: ToolSelector,
}

impl ToolRegistryV2 {
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

        Self {
            descriptors,
            selector: ToolSelector::new(),
        }
    }

    pub fn all_descriptors(&self) -> &[ToolDescriptor] {
        &self.descriptors
    }

    pub fn specs_for_mode(&self, mode: ToolMode, task_kind: TaskKind) -> Vec<ToolSpec> {
        self.selector
            .select(&mode, &task_kind, &self.descriptors)
            .into_iter()
            .map(|descriptor| descriptor.spec.clone())
            .collect()
    }
}
