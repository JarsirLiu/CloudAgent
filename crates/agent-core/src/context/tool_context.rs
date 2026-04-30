use crate::tool::ToolOutputDelta;
use std::path::PathBuf;
use tokio::sync::mpsc;
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
            output_tx: None,
        }
    }
}

#[derive(Clone)]
pub struct ToolExecutionContext {
    pub conversation_id: String,
    pub workspace_root: PathBuf,
    pub default_shell_timeout_ms: u64,
    pub cancellation_token: CancellationToken,
    pub output_tx: Option<mpsc::UnboundedSender<ToolOutputDelta>>,
}

impl ToolExecutionContext {
    pub fn with_output_tx(mut self, output_tx: mpsc::UnboundedSender<ToolOutputDelta>) -> Self {
        self.output_tx = Some(output_tx);
        self
    }
}
