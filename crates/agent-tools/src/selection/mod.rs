use super::spec::ToolDescriptor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskKind {
    RepositoryAnalysis,
    CodeEdit,
    Verification,
    WorkspaceFileOperation,
    General,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolMode {
    Explore,
    Edit,
    Verify,
    Full,
}

#[derive(Clone, Debug, Default)]
pub struct ToolSelector;

impl ToolSelector {
    pub fn new() -> Self {
        Self
    }

    pub fn select<'a>(
        &self,
        mode: &ToolMode,
        task_kind: &TaskKind,
        tools: &'a [ToolDescriptor],
    ) -> Vec<&'a ToolDescriptor> {
        tools.iter()
            .filter(|tool| matches_mode(mode, tool))
            .filter(|tool| matches_task_kind(task_kind, tool))
            .collect()
    }
}

fn matches_mode(mode: &ToolMode, tool: &ToolDescriptor) -> bool {
    match mode {
        ToolMode::Full => true,
        ToolMode::Explore => tool.mode_tags.contains(&"explore"),
        ToolMode::Edit => tool.mode_tags.contains(&"edit"),
        ToolMode::Verify => tool.mode_tags.contains(&"verify"),
    }
}

fn matches_task_kind(task_kind: &TaskKind, tool: &ToolDescriptor) -> bool {
    match task_kind {
        TaskKind::RepositoryAnalysis => {
            tool.mode_tags.contains(&"repo") || tool.mode_tags.contains(&"general")
        }
        TaskKind::CodeEdit => {
            tool.mode_tags.contains(&"edit") || tool.mode_tags.contains(&"general")
        }
        TaskKind::Verification => {
            tool.mode_tags.contains(&"verify") || tool.mode_tags.contains(&"general")
        }
        TaskKind::WorkspaceFileOperation => {
            tool.mode_tags.contains(&"fs") || tool.mode_tags.contains(&"general")
        }
        TaskKind::General => true,
    }
}
