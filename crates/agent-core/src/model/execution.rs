use super::{ChatModel, ModelRequest, ModelResponse, ModelStreamObserver};
use crate::ModelRetryStage;
use crate::TurnInterruptedError;
use anyhow::{Error, Result};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub async fn complete_model_request(
    model: &dyn ChatModel,
    cancellation_token: &CancellationToken,
    request: ModelRequest,
) -> Result<ModelResponse> {
    let mut attempt = 0u64;
    loop {
        let request_attempt = request.clone();
        let result = tokio::select! {
            _ = cancellation_token.cancelled() => {
                return Err(Error::new(TurnInterruptedError));
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
) -> Result<ModelResponse> {
    let mut attempt = 0u64;
    loop {
        let request_attempt = request.clone();
        let result = tokio::select! {
            _ = cancellation_token.cancelled() => {
                return Err(Error::new(TurnInterruptedError));
            }
            response = model.complete_streaming(request_attempt, observer) => response,
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
) -> Result<T>
where
    Fut: std::future::Future<Output = Result<T>> + Send,
{
    tokio::select! {
        _ = cancellation_token.cancelled() => {
            Err(Error::new(TurnInterruptedError))
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
#[path = "execution_tests.rs"]
mod tests;
