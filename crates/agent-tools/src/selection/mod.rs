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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSurface {
    pub mode: ToolMode,
    pub task_kind: TaskKind,
}

impl ToolSurface {
    pub fn new(mode: ToolMode, task_kind: TaskKind) -> Self {
        Self { mode, task_kind }
    }

    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            ToolMode::Explore => "explore",
            ToolMode::Edit => "edit",
            ToolMode::Verify => "verify",
            ToolMode::Full => "full",
        }
    }

    pub fn task_kind_name(&self) -> &'static str {
        match self.task_kind {
            TaskKind::RepositoryAnalysis => "repository_analysis",
            TaskKind::CodeEdit => "code_edit",
            TaskKind::Verification => "verification",
            TaskKind::WorkspaceFileOperation => "workspace_file_operation",
            TaskKind::General => "general",
        }
    }

    pub fn regular_turn() -> Self {
        Self::new(ToolMode::Full, TaskKind::General)
    }
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
        tools
            .iter()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_turn_surface_is_stable() {
        let surface = ToolSurface::regular_turn();
        assert_eq!(surface.mode, ToolMode::Full);
        assert_eq!(surface.task_kind, TaskKind::General);
    }
}
