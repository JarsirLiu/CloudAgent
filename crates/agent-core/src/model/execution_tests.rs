use super::*;
use crate::{ChatModel, ModelRetryDecision, ModelStreamObserver};
use anyhow::anyhow;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RetryTestModel {
    request_attempts: Arc<AtomicUsize>,
    stream_attempts: Arc<AtomicUsize>,
    fail_request_once: bool,
    fail_stream_once: bool,
}

#[async_trait]
impl ChatModel for RetryTestModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelResponse> {
        let attempt = self.request_attempts.fetch_add(1, Ordering::SeqCst);
        if self.fail_request_once && attempt == 0 {
            return Err(anyhow!("synthetic request transport error"));
        }
        Ok(ModelResponse {
            content: Some("ok".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: None,
            model_name: None,
            usage: None,
        })
    }

    fn request_max_retries(&self) -> u64 {
        1
    }

    fn classify_request_error(&self, err: &anyhow::Error) -> ModelRetryDecision {
        if err.to_string().contains("transport") {
            ModelRetryDecision::retry(Some(Duration::from_millis(1)))
        } else {
            ModelRetryDecision::no_retry()
        }
    }

    async fn complete_streaming(
        &self,
        _request: ModelRequest,
        observer: &mut dyn ModelStreamObserver,
    ) -> Result<ModelResponse> {
        let attempt = self.stream_attempts.fetch_add(1, Ordering::SeqCst);
        if self.fail_stream_once && attempt == 0 {
            return Err(anyhow!("synthetic stream closed before completion"));
        }
        observer.on_text_delta("ok".to_string());
        Ok(ModelResponse {
            content: Some("ok".to_string()),
            reasoning: None,
            tool_calls: Vec::new(),
            finish_reason: None,
            model_name: None,
            usage: None,
        })
    }

    fn stream_max_retries(&self) -> u64 {
        1
    }

    fn classify_stream_error(&self, err: &anyhow::Error) -> ModelRetryDecision {
        if err.to_string().contains("closed before completion") {
            ModelRetryDecision::retry(Some(Duration::from_millis(1)))
        } else {
            ModelRetryDecision::no_retry()
        }
    }
}

fn request() -> ModelRequest {
    ModelRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        temperature: 0.0,
        reasoning_effort: None,
        tool_output_token_limit: ModelRequest::default_tool_output_token_limit(),
    }
}

#[tokio::test]
async fn request_retries_once_when_retryable() {
    let model = RetryTestModel {
        request_attempts: Arc::new(AtomicUsize::new(0)),
        stream_attempts: Arc::new(AtomicUsize::new(0)),
        fail_request_once: true,
        fail_stream_once: false,
    };
    let token = CancellationToken::new();

    let response = complete_model_request(&model, &token, request())
        .await
        .expect("request should retry");

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(model.request_attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn stream_retries_once_when_retryable() {
    let model = RetryTestModel {
        request_attempts: Arc::new(AtomicUsize::new(0)),
        stream_attempts: Arc::new(AtomicUsize::new(0)),
        fail_request_once: false,
        fail_stream_once: true,
    };
    let token = CancellationToken::new();
    struct TestObserver {
        output: String,
        retries: Vec<(ModelRetryStage, u64, Duration)>,
    }
    impl ModelStreamObserver for TestObserver {
        fn on_text_delta(&mut self, delta: String) {
            self.output.push_str(&delta);
        }

        fn on_retry(&mut self, stage: ModelRetryStage, attempt: u64, delay: Duration) {
            self.retries.push((stage, attempt, delay));
        }
    }
    let mut observer = TestObserver {
        output: String::new(),
        retries: Vec::new(),
    };

    let response = complete_model_request_streaming(&model, &token, request(), &mut observer)
        .await
        .expect("stream should retry");

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(observer.output, "ok");
    assert_eq!(observer.retries.len(), 1);
    assert!(matches!(
        observer.retries[0],
        (ModelRetryStage::Streaming, 1, _)
    ));
    assert_eq!(model.stream_attempts.load(Ordering::SeqCst), 2);
}
