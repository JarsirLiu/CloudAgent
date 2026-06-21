use super::ToolRegistryOptions;
use crate::impls::command::{
    ExecCommandLocalTool, ExecCommandTool as ExecCommandDescriptorTool,
    WriteStdinTool as WriteStdinDescriptorTool,
};
use crate::impls::discovery::{ToolSearchLocalTool, ToolSearchTool, WebSearchTool};
use crate::impls::fs::{
    ApplyPatchLocalTool, ApplyPatchTool, CopyPathLocalTool, CopyPathTool, CreateDirectoryLocalTool,
    CreateDirectoryTool, CreateSkillScaffoldLocalTool, CreateSkillScaffoldTool,
    RemovePathLocalTool, RemovePathTool, UnwatchLocalTool, UnwatchTool, ValidateSkillLocalTool,
    ValidateSkillTool, WatchLocalTool, WatchManager, WatchTool,
};
use crate::registry::shared::{LocalTool, register};
use crate::spec::ToolDescriptor;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) type LocalToolMap = BTreeMap<String, Arc<dyn LocalTool>>;

pub(super) fn build_descriptors(
    _max_read_chars: usize,
    options: ToolRegistryOptions,
) -> Vec<ToolDescriptor> {
    let mut descriptors = build_main_chain_descriptors(options);
    descriptors.extend(build_platform_fs_descriptors());
    descriptors
}

fn build_main_chain_descriptors(options: ToolRegistryOptions) -> Vec<ToolDescriptor> {
    let mut descriptors = vec![
        ToolSearchTool::descriptor(),
        WebSearchTool::descriptor(),
        ExecCommandDescriptorTool::descriptor(),
        WriteStdinDescriptorTool::descriptor(),
    ];
    if options.apply_patch_enabled {
        descriptors.push(ApplyPatchTool::descriptor());
    }
    descriptors
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

pub(super) fn build_tools(_max_read_chars: usize, options: ToolRegistryOptions) -> LocalToolMap {
    let mut tools: LocalToolMap = BTreeMap::new();
    let watch_manager = WatchManager::new();
    register_main_chain_tools(&mut tools, options);
    register_platform_fs_tools(&mut tools, watch_manager);
    tools
}

fn register_main_chain_tools(tools: &mut LocalToolMap, options: ToolRegistryOptions) {
    register(tools, ToolSearchLocalTool);
    let (exec_command, write_stdin) = ExecCommandLocalTool::shared_pair();
    register(tools, exec_command);
    register(tools, write_stdin);
    if options.apply_patch_enabled {
        register(tools, ApplyPatchLocalTool);
    }
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
