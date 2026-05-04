use super::spec::ToolDescriptor;
pub use agent_core::{TaskKind, ToolMode, ToolSurface};

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
