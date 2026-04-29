use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct AgentContext {
    pub workspace_root: PathBuf,
    pub default_shell_timeout_ms: u64,
}

impl AgentContext {
    pub fn tool_context(
        &self,
        conversation_id: impl Into<String>,
        cancellation_token: CancellationToken,
    ) -> ToolExecutionContext {
        ToolExecutionContext {
            conversation_id: conversation_id.into(),
            workspace_root: self.workspace_root.clone(),
            default_shell_timeout_ms: self.default_shell_timeout_ms,
            cancellation_token,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolExecutionContext {
    pub conversation_id: String,
    pub workspace_root: PathBuf,
    pub default_shell_timeout_ms: u64,
    pub cancellation_token: CancellationToken,
}
