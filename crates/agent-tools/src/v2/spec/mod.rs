use agent_core::ToolSpec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCategory {
    RepositoryExploration,
    CodeEditing,
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

#[derive(Clone, Debug)]
pub struct ToolDescriptor {
    pub category: ToolCategory,
    pub risk: ToolRisk,
    pub mode_tags: Vec<&'static str>,
    pub spec: ToolSpec,
}

impl ToolDescriptor {
    pub fn new(
        category: ToolCategory,
        risk: ToolRisk,
        mode_tags: Vec<&'static str>,
        spec: ToolSpec,
    ) -> Self {
        Self {
            category,
            risk,
            mode_tags,
            spec,
        }
    }
}
