use super::selection::ToolMode;

#[derive(Clone, Debug)]
pub struct ToolPolicy {
    pub max_directory_only_rounds: usize,
    pub encourage_batch_reads: bool,
    pub default_mode: ToolMode,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            max_directory_only_rounds: 2,
            encourage_batch_reads: true,
            default_mode: ToolMode::Explore,
        }
    }
}
