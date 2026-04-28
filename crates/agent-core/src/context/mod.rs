use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct AgentContext {
    pub workspace_root: PathBuf,
    pub default_shell_timeout_ms: u64,
}

impl AgentContext {
    pub fn tool_context(&self, session_id: impl Into<String>) -> ToolExecutionContext {
        ToolExecutionContext {
            session_id: session_id.into(),
            workspace_root: self.workspace_root.clone(),
            default_shell_timeout_ms: self.default_shell_timeout_ms,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolExecutionContext {
    pub session_id: String,
    pub workspace_root: PathBuf,
    pub default_shell_timeout_ms: u64,
}

pub fn module_name() -> &'static str {
    "agent-core::context"
}
