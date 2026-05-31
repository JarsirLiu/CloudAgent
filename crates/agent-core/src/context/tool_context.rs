use crate::PermissionProfile;
use crate::output_truncation::DEFAULT_MAX_OUTPUT_TOKENS;
use crate::tool::{ToolOutputDelta, ToolSpec};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct AgentContext {
    pub workspace_root: PathBuf,
    pub data_root_dir: PathBuf,
    pub conversation_store_dir: PathBuf,
    pub default_shell_timeout_ms: u64,
    pub tool_output_token_limit: usize,
}

impl AgentContext {
    pub fn tool_context(
        &self,
        conversation_id: impl Into<String>,
        permission_profile: PermissionProfile,
        cancellation_token: CancellationToken,
        discoverable_tools: Vec<ToolSpec>,
    ) -> ToolExecutionContext {
        ToolExecutionContext {
            conversation_id: conversation_id.into(),
            workspace_root: self.workspace_root.clone(),
            conversation_store_dir: self.conversation_store_dir.clone(),
            permission_profile,
            default_shell_timeout_ms: self.default_shell_timeout_ms,
            max_tool_output_tokens: self.tool_output_token_limit.max(1),
            cancellation_token,
            discoverable_tools,
            output_tx: None,
        }
    }
}

#[derive(Clone)]
pub struct ToolExecutionContext {
    pub conversation_id: String,
    pub workspace_root: PathBuf,
    pub conversation_store_dir: PathBuf,
    pub permission_profile: PermissionProfile,
    pub default_shell_timeout_ms: u64,
    pub max_tool_output_tokens: usize,
    pub cancellation_token: CancellationToken,
    pub discoverable_tools: Vec<ToolSpec>,
    pub output_tx: Option<mpsc::UnboundedSender<ToolOutputDelta>>,
}

impl ToolExecutionContext {
    pub fn default_max_tool_output_tokens() -> usize {
        DEFAULT_MAX_OUTPUT_TOKENS
    }

    pub fn with_output_tx(mut self, output_tx: mpsc::UnboundedSender<ToolOutputDelta>) -> Self {
        self.output_tx = Some(output_tx);
        self
    }
}
