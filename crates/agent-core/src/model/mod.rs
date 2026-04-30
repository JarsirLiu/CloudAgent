use crate::conversation::ResponseItem;
use crate::tool::{ToolCall, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
    pub usage: Option<ModelUsage>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub total_tokens: u64,
}

impl ModelUsage {
    pub fn add_assign(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.cached_input_tokens = self
            .cached_input_tokens
            .saturating_add(other.cached_input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.reasoning_output_tokens = self
            .reasoning_output_tokens
            .saturating_add(other.reasoning_output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
    }
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
