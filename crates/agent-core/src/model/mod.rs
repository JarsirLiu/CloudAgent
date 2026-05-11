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
    pub reasoning: Option<String>,
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
    pub fn total_output_tokens(&self) -> u64 {
        self.output_tokens
            .saturating_add(self.reasoning_output_tokens)
    }

    pub fn total_consumed_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.reasoning_output_tokens)
    }

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReasoningDelta {
    SummaryText { summary_index: usize, delta: String },
    Text { content_index: usize, delta: String },
}

pub trait ModelStreamObserver: Send {
    fn on_text_delta(&mut self, delta: String);

    fn on_reasoning_delta(&mut self, _delta: ReasoningDelta) {}

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
        observer: &mut dyn ModelStreamObserver,
    ) -> Result<ModelResponse> {
        let response = self.complete(request).await?;
        if let Some(reasoning) = response.reasoning.clone() {
            observer.on_reasoning_delta(ReasoningDelta::Text {
                content_index: 0,
                delta: reasoning,
            });
        }
        if let Some(content) = response.content.clone() {
            observer.on_text_delta(content);
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::ModelUsage;

    #[test]
    fn total_output_tokens_includes_reasoning_tokens() {
        let usage = ModelUsage {
            input_tokens: 1,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_output_tokens: 4,
            total_tokens: 10,
        };

        assert_eq!(usage.total_output_tokens(), 7);
    }

    #[test]
    fn total_consumed_tokens_includes_reasoning_and_input() {
        let usage = ModelUsage {
            input_tokens: 11,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_output_tokens: 4,
            total_tokens: 99,
        };

        assert_eq!(usage.total_consumed_tokens(), 18);
    }
}
