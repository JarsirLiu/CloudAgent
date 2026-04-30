use crate::conversation::ResponseItem;
use crate::tool::{ToolCall, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct ModelRequest {
    pub messages: Vec<ResponseItem>,
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

    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_text_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ModelResponse> {
        let response = self.complete(request).await?;
        if let Some(content) = response.content.clone() {
            on_text_delta(content);
        }
        Ok(response)
    }
}

pub fn module_name() -> &'static str {
    "agent-core::model"
}
