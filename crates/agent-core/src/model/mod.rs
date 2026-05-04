use crate::ModelRetryStage;
use crate::conversation::ResponseItem;
use crate::tool::{ToolCall, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

mod execution;

pub use execution::{
    await_server_request_decision, complete_model_request, complete_model_request_streaming,
};

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelRetryDecision {
    pub retryable: bool,
    pub delay: Option<Duration>,
}

impl ModelRetryDecision {
    pub fn no_retry() -> Self {
        Self::default()
    }

    pub fn retry(delay: Option<Duration>) -> Self {
        Self {
            retryable: true,
            delay,
        }
    }
}

pub trait ModelStreamObserver: Send {
    fn on_text_delta(&mut self, delta: String);

    fn on_retry(&mut self, _stage: ModelRetryStage, _attempt: u64, _delay: Duration) {}
}

#[async_trait]
pub trait ChatModel: Send + Sync {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse>;

    fn request_max_retries(&self) -> u64 {
        0
    }

    fn stream_max_retries(&self) -> u64 {
        0
    }

    fn classify_request_error(&self, _err: &anyhow::Error) -> ModelRetryDecision {
        ModelRetryDecision::no_retry()
    }

    fn classify_stream_error(&self, _err: &anyhow::Error) -> ModelRetryDecision {
        ModelRetryDecision::no_retry()
    }

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
