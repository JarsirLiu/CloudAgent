use crate::impls::command::{ExecCommandLocalTool, ExecCommandTool as ExecCommandDescriptorTool};
use crate::impls::file_read_state::FileReadStateStore;
use crate::impls::fs::{EditFileLocalTool, EditFileTool, GetMetadataLocalTool, GetMetadataTool};
use crate::impls::repo::{
    ListDirectoryLocalTool, ListDirectoryTool, ReadFilesLocalTool, ReadFilesTool,
    SearchWorkspaceLocalTool, SearchWorkspaceTool,
};
use crate::registry::shared::{LocalTool, register};
use crate::selection::ToolSelector;
use crate::spec::ToolDescriptor;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) type LocalToolMap = BTreeMap<String, Arc<dyn LocalTool>>;

pub(super) fn build_descriptors(max_read_chars: usize) -> Vec<ToolDescriptor> {
    vec![
        SearchWorkspaceTool::descriptor(),
        ListDirectoryTool::descriptor(),
        ReadFilesTool::descriptor(max_read_chars),
        ExecCommandDescriptorTool::descriptor(),
        EditFileTool::descriptor(),
        GetMetadataTool::descriptor(),
    ]
}

pub(super) fn build_tools(max_read_chars: usize) -> LocalToolMap {
    let mut tools: LocalToolMap = BTreeMap::new();
    let read_state = FileReadStateStore::new();
    register(&mut tools, SearchWorkspaceLocalTool::new());
    register(&mut tools, ListDirectoryLocalTool);
    register(
        &mut tools,
        ReadFilesLocalTool {
            max_read_chars,
            read_state: read_state.clone(),
        },
    );
    register(&mut tools, ExecCommandLocalTool::new());
    register(&mut tools, EditFileLocalTool { read_state });
    register(&mut tools, GetMetadataLocalTool);
    tools
}

pub(super) fn build_selector() -> ToolSelector {
    ToolSelector::new()
}
