use crate::spec::ToolDescriptor;
use agent_core::{
    PermissionProfile, ResolvedToolSet, ToolBatchExecutionStrategy, ToolCall, ToolSpec,
};
use std::collections::BTreeSet;

pub(super) fn resolve_surface<'a>(
    descriptors: impl IntoIterator<Item = &'a ToolDescriptor>,
    permission_profile: &PermissionProfile,
) -> ResolvedToolSet {
    let mut specs = Vec::new();
    let mut parallel_tool_names = BTreeSet::new();

    for descriptor in descriptors {
        if !descriptor.min_permission.allows(permission_profile) {
            continue;
        }
        if descriptor.supports_parallel_calls {
            parallel_tool_names.insert(descriptor.spec.identity.wire_name.clone());
        }
        specs.push(descriptor.spec.clone());
    }

    let mut resolved = ResolvedToolSet::new(specs);
    for tool_name in parallel_tool_names {
        resolved.mark_parallel_tool(tool_name);
    }
    resolved
}

pub(super) fn specs_for_surface<'a>(
    descriptors: impl IntoIterator<Item = &'a ToolDescriptor>,
) -> Vec<ToolSpec> {
    descriptors
        .into_iter()
        .map(|descriptor| descriptor.spec.clone())
        .collect()
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
            .is_some_and(|descriptor| {
                !descriptor.spec.mutating && descriptor.supports_parallel_calls
            })
    });

    if all_parallel_safe {
        ToolBatchExecutionStrategy::Parallel
    } else {
        ToolBatchExecutionStrategy::Sequential
    }
}
