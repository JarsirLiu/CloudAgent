use agent_core::ToolSpec;
use agent_protocol::PermissionProfile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCategory {
    RepositoryExploration,
    CommandExecution,
    WorkspaceFileOps,
    ExternalResources,
    AgentCoordination,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolPermissionTier {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

impl ToolPermissionTier {
    pub fn allows(&self, profile: &PermissionProfile) -> bool {
        let granted = match profile {
            PermissionProfile::ReadOnly => Self::ReadOnly,
            PermissionProfile::WorkspaceWrite => Self::WorkspaceWrite,
            PermissionProfile::FullAccess => Self::FullAccess,
        };
        granted >= *self
    }
}

#[derive(Clone, Debug)]
pub struct ToolDescriptor {
    pub category: ToolCategory,
    pub risk: ToolRisk,
    pub min_permission: ToolPermissionTier,
    pub supports_parallel_calls: bool,
    pub mode_tags: Vec<&'static str>,
    pub spec: ToolSpec,
}

impl ToolDescriptor {
    pub fn new(
        category: ToolCategory,
        risk: ToolRisk,
        min_permission: ToolPermissionTier,
        supports_parallel_calls: bool,
        mode_tags: Vec<&'static str>,
        spec: ToolSpec,
    ) -> Self {
        Self {
            category,
            risk,
            min_permission,
            supports_parallel_calls,
            mode_tags,
            spec,
        }
    }
}
