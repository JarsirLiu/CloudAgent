use crate::registry::mcp::McpToolDescriptor;
use crate::spec::{
    ToolDefaultVisibility, ToolDescriptor, ToolEnvironmentRequirement, ToolPermissionTier,
};
use agent_core::{PermissionProfile, ToolBatchExecutionStrategy, ToolCall, ToolSpec};

#[derive(Clone, Debug)]
pub(super) struct RegisteredTool {
    pub(super) spec: ToolSpec,
    pub(super) min_permission: ToolPermissionTier,
    pub(super) default_visibility: ToolDefaultVisibility,
    pub(super) selection_priority: i32,
    pub(super) environment_visible: bool,
}

pub(super) fn registered_tools(
    descriptors: &[ToolDescriptor],
    mcp_descriptors: Vec<McpToolDescriptor>,
    mcp_environment_visible: bool,
    discoverable_tools_present: bool,
) -> Vec<RegisteredTool> {
    let built_ins = descriptors.iter().map(|descriptor| RegisteredTool {
        spec: descriptor.spec.clone(),
        min_permission: descriptor.min_permission.clone(),
        default_visibility: descriptor.default_visibility.clone(),
        selection_priority: descriptor.usage.selection_priority,
        environment_visible: match descriptor.environment_requirement {
            ToolEnvironmentRequirement::Always => true,
            ToolEnvironmentRequirement::RequiresDiscoverableTools => discoverable_tools_present,
        },
    });
    let mcp = mcp_descriptors.into_iter().map(|descriptor| RegisteredTool {
        spec: descriptor.spec,
        min_permission: descriptor.min_permission,
        default_visibility: descriptor.default_visibility,
        selection_priority: descriptor.selection_priority,
        environment_visible: mcp_environment_visible,
    });

    built_ins.chain(mcp).collect()
}

pub(super) fn environment_visible_tools(
    tools: Vec<RegisteredTool>,
) -> Vec<RegisteredTool> {
    tools.into_iter()
        .filter(|tool| tool.environment_visible)
        .collect()
}

pub(super) fn permission_allowed_tools(
    tools: impl IntoIterator<Item = RegisteredTool>,
    permission_profile: &PermissionProfile,
) -> Vec<RegisteredTool> {
    tools.into_iter()
        .filter(|tool| tool.min_permission.allows(permission_profile))
        .collect()
}

pub(super) fn model_visible_default_tools(
    tools: impl IntoIterator<Item = RegisteredTool>,
) -> Vec<RegisteredTool> {
    let mut tools = tools
        .into_iter()
        .filter(|tool| tool.default_visibility == ToolDefaultVisibility::Default)
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| {
        right
            .selection_priority
            .cmp(&left.selection_priority)
            .then_with(|| left.spec.name.cmp(&right.spec.name))
    });
    tools
}

pub(super) fn deferred_discoverable_tools(
    tools: impl IntoIterator<Item = RegisteredTool>,
) -> Vec<RegisteredTool> {
    let mut tools = tools
        .into_iter()
        .filter(|tool| tool.default_visibility == ToolDefaultVisibility::Deferred)
        .collect::<Vec<_>>();
    tools.sort_by(|left, right| {
        right
            .selection_priority
            .cmp(&left.selection_priority)
            .then_with(|| left.spec.name.cmp(&right.spec.name))
    });
    tools
}

pub(super) fn specs_for_registered_tools(
    tools: impl IntoIterator<Item = RegisteredTool>,
) -> Vec<ToolSpec> {
    tools.into_iter().map(|tool| tool.spec).collect()
}

pub(super) fn batch_execution_strategy(
    descriptors: &[ToolDescriptor],
    calls: &[ToolCall],
) -> ToolBatchExecutionStrategy {
    if calls.len() <= 1 {
        return ToolBatchExecutionStrategy::Sequential;
    }

    let all_parallel_safe = calls.iter().all(|call| {
        descriptors
            .iter()
            .find(|descriptor| descriptor.spec.identity.wire_name == call.identity.wire_name)
            .is_some_and(|descriptor| descriptor.spec.execution_policy.supports_parallel())
    });

    if all_parallel_safe {
        ToolBatchExecutionStrategy::Parallel
    } else {
        ToolBatchExecutionStrategy::Sequential
    }
}
