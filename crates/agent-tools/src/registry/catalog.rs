use crate::impls::command::{ExecCommandLocalTool, ExecCommandTool as ExecCommandDescriptorTool};
use crate::impls::discovery::{ToolSearchLocalTool, ToolSearchTool};
use crate::impls::file_read_state::FileReadStateStore;
use crate::impls::fs::{
    CopyPathLocalTool, CopyPathTool, CreateDirectoryLocalTool, CreateDirectoryTool,
    EditFileLocalTool, EditFileTool, GetMetadataLocalTool, GetMetadataTool, ReadDirectoryLocalTool,
    ReadDirectoryTool, ReadFileBytesLocalTool, ReadFileBytesTool, RemovePathLocalTool,
    RemovePathTool, UnwatchLocalTool, UnwatchTool, WatchLocalTool, WatchManager, WatchTool,
    WriteFileBytesLocalTool, WriteFileBytesTool,
};
use crate::impls::repo::{
    ReadFileLocalTool, ReadFileTool, SearchWorkspaceLocalTool, SearchWorkspaceTool,
};
use crate::registry::shared::{LocalTool, register};
use crate::spec::ToolDescriptor;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) type LocalToolMap = BTreeMap<String, Arc<dyn LocalTool>>;

pub(super) fn build_descriptors(max_read_chars: usize) -> Vec<ToolDescriptor> {
    vec![
        SearchWorkspaceTool::descriptor(),
        ReadFileTool::descriptor(max_read_chars),
        ToolSearchTool::descriptor(),
        ExecCommandDescriptorTool::descriptor(),
        EditFileTool::descriptor(),
        GetMetadataTool::descriptor(),
        ReadDirectoryTool::descriptor(),
        CreateDirectoryTool::descriptor(),
        CopyPathTool::descriptor(),
        RemovePathTool::descriptor(),
        ReadFileBytesTool::descriptor(),
        WriteFileBytesTool::descriptor(),
        WatchTool::descriptor(),
        UnwatchTool::descriptor(),
    ]
}

pub(super) fn build_tools(max_read_chars: usize) -> LocalToolMap {
    let mut tools: LocalToolMap = BTreeMap::new();
    let read_state = FileReadStateStore::new();
    let watch_manager = WatchManager::new();
    register(&mut tools, SearchWorkspaceLocalTool::new());
    register(
        &mut tools,
        ReadFileLocalTool {
            max_read_chars,
            read_state: read_state.clone(),
        },
    );
    register(&mut tools, ToolSearchLocalTool);
    register(&mut tools, ExecCommandLocalTool::new());
    register(&mut tools, EditFileLocalTool { read_state });
    register(&mut tools, GetMetadataLocalTool);
    register(&mut tools, ReadDirectoryLocalTool);
    register(&mut tools, CreateDirectoryLocalTool);
    register(&mut tools, CopyPathLocalTool);
    register(&mut tools, RemovePathLocalTool);
    register(&mut tools, ReadFileBytesLocalTool);
    register(&mut tools, WriteFileBytesLocalTool);
    register(
        &mut tools,
        WatchLocalTool {
            manager: watch_manager.clone(),
        },
    );
    register(
        &mut tools,
        UnwatchLocalTool {
            manager: watch_manager,
        },
    );
    tools
}
