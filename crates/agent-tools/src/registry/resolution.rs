use crate::spec::ToolDescriptor;
use agent_core::{
    PermissionProfile, ResolvedToolSet, ToolBatchExecutionStrategy, ToolCall, ToolSpec,
};

pub(super) fn resolve_surface<'a>(
    descriptors: impl IntoIterator<Item = &'a ToolDescriptor>,
    permission_profile: &PermissionProfile,
) -> ResolvedToolSet {
    let mut specs = Vec::new();

    for descriptor in descriptors {
        if !descriptor.min_permission.allows(permission_profile) {
            continue;
        }
        specs.push(descriptor.spec.clone());
    }

    ResolvedToolSet::new(specs)
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
            .is_some_and(|descriptor| descriptor.spec.execution_policy.supports_parallel())
    });

    if all_parallel_safe {
        ToolBatchExecutionStrategy::Parallel
    } else {
        ToolBatchExecutionStrategy::Sequential
    }
}
