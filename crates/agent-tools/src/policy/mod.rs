mod approval;

use super::selection::ToolMode;
pub use approval::{ApprovalRequirement, approval_requirement_for_tool};

#[derive(Clone, Debug)]
pub struct SearchPolicy {
    pub respect_gitignore: bool,
    pub ignored_directory_names: Vec<&'static str>,
}

impl Default for SearchPolicy {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            ignored_directory_names: vec![
                ".git",
                ".hg",
                ".svn",
                "node_modules",
                "dist",
                "build",
                "target",
                "target-verify",
                ".next",
                ".nuxt",
                ".turbo",
                ".cache",
                "coverage",
                ".venv",
                "venv",
                "__pycache__",
            ],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolPolicy {
    pub max_directory_only_rounds: usize,
    pub encourage_batch_reads: bool,
    pub default_mode: ToolMode,
    pub search: SearchPolicy,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            max_directory_only_rounds: 2,
            encourage_batch_reads: true,
            default_mode: ToolMode::Explore,
            search: SearchPolicy::default(),
        }
    }
}
