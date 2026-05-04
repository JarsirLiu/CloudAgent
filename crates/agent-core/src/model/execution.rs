use super::{ChatModel, ModelRequest, ModelResponse, ModelStreamObserver};
use crate::ModelRetryStage;
use anyhow::{Result, anyhow};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub async fn complete_model_request(
    model: &dyn ChatModel,
    cancellation_token: &CancellationToken,
    request: ModelRequest,
    interrupted_error: &str,
) -> Result<ModelResponse> {
    let mut attempt = 0u64;
    loop {
        let request_attempt = request.clone();
        let result = tokio::select! {
            _ = cancellation_token.cancelled() => {
                return Err(anyhow!(interrupted_error.to_string()));
            }
            response = model.complete(request_attempt) => response,
        };

        match result {
            Ok(response) => return Ok(response),
            Err(err) => {
                let decision = model.classify_request_error(&err);
                if !decision.retryable || attempt >= model.request_max_retries() {
                    return Err(err);
                }
                attempt += 1;
                tokio::time::sleep(retry_delay(decision.delay, attempt)).await;
            }
        }
    }
}

pub async fn complete_model_request_streaming(
    model: &dyn ChatModel,
    cancellation_token: &CancellationToken,
    request: ModelRequest,
    observer: &mut dyn ModelStreamObserver,
    interrupted_error: &str,
) -> Result<ModelResponse> {
    let mut attempt = 0u64;
    loop {
        let request_attempt = request.clone();
        let mut on_text_delta = |delta| observer.on_text_delta(delta);
        let result = tokio::select! {
            _ = cancellation_token.cancelled() => {
                return Err(anyhow!(interrupted_error.to_string()));
            }
            response = model.complete_streaming(request_attempt, &mut on_text_delta) => response,
        };

        match result {
            Ok(response) => return Ok(response),
            Err(err) => {
                let decision = model.classify_stream_error(&err);
                if !decision.retryable || attempt >= model.stream_max_retries() {
                    return Err(err);
                }
                attempt += 1;
                let delay = retry_delay(decision.delay, attempt);
                observer.on_retry(ModelRetryStage::Streaming, attempt, delay);
                tokio::time::sleep(delay).await;
            }
        }
    }
}

pub async fn await_server_request_decision<T, Fut>(
    cancellation_token: &CancellationToken,
    decision_future: Fut,
    interrupted_error: &str,
) -> Result<T>
where
    Fut: std::future::Future<Output = Result<T>> + Send,
{
    tokio::select! {
        _ = cancellation_token.cancelled() => {
            return Err(anyhow!(interrupted_error.to_string()));
        }
        response = decision_future => response,
    }
}

fn retry_delay(explicit_delay: Option<Duration>, attempt: u64) -> Duration {
    explicit_delay.unwrap_or_else(|| {
        let millis = 250u64.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1) as u32));
        Duration::from_millis(millis.min(5_000))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatModel, ModelRetryDecision, ModelStreamObserver};
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
                tool_calls: Vec::new(),
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
            on_text_delta: &mut (dyn FnMut(String) + Send),
        ) -> Result<ModelResponse> {
            let attempt = self.stream_attempts.fetch_add(1, Ordering::SeqCst);
            if self.fail_stream_once && attempt == 0 {
                return Err(anyhow!("synthetic stream closed before completion"));
            }
            on_text_delta("ok".to_string());
            Ok(ModelResponse {
                content: Some("ok".to_string()),
                tool_calls: Vec::new(),
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

        let response = complete_model_request(&model, &token, request(), "interrupted")
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

        let response = complete_model_request_streaming(
            &model,
            &token,
            request(),
            &mut observer,
            "interrupted",
        )
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
}
