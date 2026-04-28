use crate::context::ToolExecutionContext;
pub use agent_protocol::{ToolCall, ToolResult, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct ToolEvent {
    pub name: String,
    pub summary: String,
    pub is_error: bool,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn specs(&self) -> Vec<ToolSpec>;

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult>;
}

pub fn module_name() -> &'static str {
    "agent-core::tool"
}
