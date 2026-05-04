use super::{ChatModel, ModelRequest, ModelResponse};
use anyhow::{Result, anyhow};
use tokio_util::sync::CancellationToken;

pub async fn complete_model_request(
    model: &dyn ChatModel,
    cancellation_token: &CancellationToken,
    request: ModelRequest,
    interrupted_error: &str,
) -> Result<ModelResponse> {
    tokio::select! {
        _ = cancellation_token.cancelled() => {
            return Err(anyhow!(interrupted_error.to_string()));
        }
        response = model.complete(request) => response,
    }
}

pub async fn complete_model_request_streaming(
    model: &dyn ChatModel,
    cancellation_token: &CancellationToken,
    request: ModelRequest,
    on_text_delta: &mut (dyn FnMut(String) + Send),
    interrupted_error: &str,
) -> Result<ModelResponse> {
    tokio::select! {
        _ = cancellation_token.cancelled() => {
            return Err(anyhow!(interrupted_error.to_string()));
        }
        response = model.complete_streaming(request, on_text_delta) => response,
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
