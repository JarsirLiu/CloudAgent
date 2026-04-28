use crate::context::ToolExecutionContext;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub mutating: bool,
    pub requires_approval: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
    pub summary: String,
    pub is_error: bool,
}

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
