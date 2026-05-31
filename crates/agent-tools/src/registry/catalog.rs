use crate::impls::command::{
    ExecCommandLocalTool, ExecCommandTool as ExecCommandDescriptorTool,
    WriteStdinTool as WriteStdinDescriptorTool,
};
use crate::impls::discovery::{ToolSearchLocalTool, ToolSearchTool};
use crate::impls::file_read_state::FileReadStateStore;
use crate::impls::fs::{
    CopyPathLocalTool, CopyPathTool, CreateDirectoryLocalTool, CreateDirectoryTool,
    CreateSkillScaffoldLocalTool, CreateSkillScaffoldTool, EditFileLocalTool, EditFileTool,
    RemovePathLocalTool, RemovePathTool, UnwatchLocalTool, UnwatchTool, ValidateSkillLocalTool,
    ValidateSkillTool, WatchLocalTool, WatchManager, WatchTool,
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
    let mut descriptors = build_main_chain_descriptors(max_read_chars);
    descriptors.extend(build_platform_fs_descriptors());
    descriptors
}

fn build_main_chain_descriptors(max_read_chars: usize) -> Vec<ToolDescriptor> {
    vec![
        SearchWorkspaceTool::descriptor(),
        ReadFileTool::descriptor(max_read_chars),
        ToolSearchTool::descriptor(),
        ExecCommandDescriptorTool::descriptor(),
        WriteStdinDescriptorTool::descriptor(),
        EditFileTool::descriptor(),
    ]
}

fn build_platform_fs_descriptors() -> Vec<ToolDescriptor> {
    vec![
        CreateDirectoryTool::descriptor(),
        CreateSkillScaffoldTool::descriptor(),
        CopyPathTool::descriptor(),
        RemovePathTool::descriptor(),
        ValidateSkillTool::descriptor(),
        WatchTool::descriptor(),
        UnwatchTool::descriptor(),
    ]
}

pub(super) fn build_tools(max_read_chars: usize) -> LocalToolMap {
    let mut tools: LocalToolMap = BTreeMap::new();
    let read_state = FileReadStateStore::new();
    let watch_manager = WatchManager::new();
    register_main_chain_tools(&mut tools, max_read_chars, read_state.clone());
    register_platform_fs_tools(&mut tools, watch_manager);
    tools
}

fn register_main_chain_tools(
    tools: &mut LocalToolMap,
    max_read_chars: usize,
    read_state: FileReadStateStore,
) {
    register(tools, SearchWorkspaceLocalTool::new());
    register(
        tools,
        ReadFileLocalTool {
            max_read_chars,
            read_state: read_state.clone(),
        },
    );
    register(tools, ToolSearchLocalTool);
    let (exec_command, write_stdin) = ExecCommandLocalTool::shared_pair();
    register(tools, exec_command);
    register(tools, write_stdin);
    register(tools, EditFileLocalTool { read_state });
}

fn register_platform_fs_tools(tools: &mut LocalToolMap, watch_manager: WatchManager) {
    register(tools, CreateDirectoryLocalTool);
    register(tools, CreateSkillScaffoldLocalTool);
    register(tools, CopyPathLocalTool);
    register(tools, RemovePathLocalTool);
    register(tools, ValidateSkillLocalTool);
    register(
        tools,
        WatchLocalTool {
            manager: watch_manager.clone(),
        },
    );
    register(
        tools,
        UnwatchLocalTool {
            manager: watch_manager,
        },
    );
}
