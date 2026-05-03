use crate::spec::ToolDescriptor;
use agent_core::{ToolCall, ToolSpec};
use agent_protocol::PermissionProfile;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct ResolvedToolSet {
    pub specs: Vec<ToolSpec>,
    parallel_tool_names: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolBatchExecutionStrategy {
    Sequential,
    Parallel,
}

impl ResolvedToolSet {
    pub fn supports_parallel_tool(&self, tool_name: &str) -> bool {
        self.parallel_tool_names.contains(tool_name)
    }
}

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
            parallel_tool_names.insert(descriptor.spec.name.clone());
        }
        specs.push(descriptor.spec.clone());
    }

    ResolvedToolSet {
        specs,
        parallel_tool_names,
    }
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
            .find(|descriptor| descriptor.spec.name == call.name)
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
