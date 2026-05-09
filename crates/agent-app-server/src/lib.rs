mod app;
mod projection;
mod routing;
mod server_request;
mod session;
pub mod transport;
mod turn;

use agent_core::AgentHost;
use agent_protocol::{
    AppClientCommandEnvelope, AppServerMessageEnvelope, JsonRpcError, JsonRpcErrorPayload,
    JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

pub use app::in_process::{
    InProcessClientHandle, InProcessClientSender, InProcessServer, start_in_process,
};

pub async fn run_stdio_server(
    runtime: Arc<AgentHost>,
    conversation_id: String,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    let mut client = start_in_process(runtime.clone(), conversation_id.clone(), auto_approve, auto_approve_reason);
    let sender = client.sender();
    let state = client.state();
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<JsonRpcMessage>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<JsonRpcMessage>();
    let event_tx_for_events = event_tx.clone();

    let read_task = tokio::spawn(async move { transport::stdio::read_messages(command_tx).await });
    let write_task = tokio::spawn(async move { transport::stdio::write_messages(event_rx).await });
    let forward_events = tokio::spawn(async move {
        let mut seq_by_conversation: HashMap<String, u64> = HashMap::new();
        while let Some(message) = client.next_message().await {
            let event_seq = message.conversation_id().map(|conversation_id| {
                let next = seq_by_conversation
                    .entry(conversation_id.to_string())
                    .or_insert(0);
                *next += 1;
                *next
            });
            if event_tx_for_events
                .send(JsonRpcMessage::from(AppServerMessageEnvelope { message, event_seq }))
                .is_err()
            {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    let forward_commands = tokio::spawn(async move {
        while let Some(message) = command_rx.recv().await {
            match message {
                JsonRpcMessage::Request(request) => {
                    handle_stdio_request(&runtime, &sender, &event_tx, &state, request).await?;
                }
                JsonRpcMessage::Notification(notification) => {
                    let envelope =
                        AppClientCommandEnvelope::try_from(JsonRpcMessage::Notification(notification))?;
                    sender.send_command(envelope.command)?;
                }
                JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {}
            }
        }
        Ok::<(), anyhow::Error>(())
    });

    read_task.await??;
    forward_commands.await??;
    forward_events.await??;
    write_task.await??;
    Ok(())
}

async fn handle_stdio_request(
    runtime: &Arc<AgentHost>,
    sender: &app::in_process::InProcessClientSender,
    event_tx: &mpsc::UnboundedSender<JsonRpcMessage>,
    state: &Arc<tokio::sync::Mutex<routing::command_router::ServerState>>,
    request: JsonRpcRequest,
) -> Result<()> {
    let request_id = request.id.clone();
    let response = match request.method.as_str() {
        "conversation/list" => {
            let result = session::service::read_conversation_list(runtime, state).await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "conversation/status" => {
            let conversation_id = required_string_param(&request, "conversation_id")?;
            let result =
                session::service::read_conversation_status(runtime, state, conversation_id).await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "conversation/history" => {
            let conversation_id = required_string_param(&request, "conversation_id")?;
            let result =
                session::service::read_conversation_history(runtime, state, conversation_id).await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "conversation/historyPage" => {
            let params = request.params.clone().unwrap_or(serde_json::Value::Null);
            let conversation_id = value_field::<String>(&params, "conversation_id")?;
            let before_turn_id = optional_value_field::<String>(&params, "before_turn_id")?;
            let limit = optional_value_field::<usize>(&params, "limit")?.unwrap_or(30);
            let result = session::service::read_conversation_history_page(
                runtime,
                state,
                conversation_id,
                before_turn_id,
                limit,
            )
            .await?;
            JsonRpcMessage::Response(JsonRpcResponse {
                id: request_id,
                result: serde_json::to_value(result)?,
            })
        }
        "hub/node/list" | "hub/node/select" => JsonRpcMessage::Error(JsonRpcError {
            id: request_id,
            error: JsonRpcErrorPayload {
                code: -32601,
                message: format!(
                    "{} is not available for the current direct target",
                    request.method
                ),
                data: None,
            },
        }),
        _ => match AppClientCommandEnvelope::try_from(JsonRpcMessage::Request(request)) {
            Ok(envelope) => {
                sender.send_command(envelope.command)?;
                return Ok(());
            }
            Err(error) => {
                let code = if error.to_string().contains("unsupported request method") {
                    -32601
                } else {
                    -32602
                };
                JsonRpcMessage::Error(JsonRpcError {
                    id: request_id,
                    error: JsonRpcErrorPayload {
                        code,
                        message: error.to_string(),
                        data: None,
                    },
                })
            }
        },
    };

    let _ = event_tx.send(response);
    Ok(())
}

fn required_string_param(request: &JsonRpcRequest, field: &str) -> Result<String> {
    let params = request.params.clone().unwrap_or(serde_json::Value::Null);
    value_field::<String>(&params, field)
}

fn value_field<T>(params: &serde_json::Value, field: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let value = params
        .get(field)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing required field `{field}`"))?;
    Ok(serde_json::from_value(value)?)
}

fn optional_value_field<T>(params: &serde_json::Value, field: &str) -> Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    let Some(value) = params.get(field).cloned() else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(value)?))
}
