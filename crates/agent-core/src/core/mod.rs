use crate::session::ConversationMessage;
use crate::tool::{ToolCall, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct ModelRequest {
    pub messages: Vec<ConversationMessage>,
    pub tools: Vec<ToolSpec>,
    pub temperature: f32,
}

#[derive(Clone, Debug)]
pub struct ModelResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub model_name: Option<String>,
}

#[async_trait]
pub trait ChatModel: Send + Sync {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse>;
}

pub fn module_name() -> &'static str {
    "agent-core::core"
}
