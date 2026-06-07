use crate::node::runtime::NodeRuntime;
use crate::node::worker_manager::NodeEvent;
use agent_protocol::{
    AppServerMessage, AppServerMessageEnvelope, AppServerNotification, JsonRpcError,
    JsonRpcErrorPayload, JsonRpcMessage, JsonRpcResponse, RequestId,
};
use anyhow::Result;
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub(crate) async fn write_node_event<W>(
    writer: &mut W,
    event: NodeEvent,
    runtime: &NodeRuntime,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message = match event {
        NodeEvent::Message { message } => *message,
        NodeEvent::Diagnostic {
            conversation_id,
            message,
            is_error,
        } => AppServerMessage::Notification(if is_error {
            AppServerNotification::Error {
                conversation_id,
                message,
            }
        } else {
            AppServerNotification::Info {
                conversation_id,
                message,
            }
        }),
    };
    sync_registry_from_message(runtime, &message).await;
    write_app_server_message(writer, message).await
}

pub(crate) async fn sync_registry_from_message(runtime: &NodeRuntime, message: &AppServerMessage) {
    let AppServerMessage::Notification(notification) = message else {
        return;
    };

    sync_execution_registry_from_notification(runtime, notification).await;

    let mut registry = runtime.conversations().lock().await;
    match notification {
        AppServerNotification::ConversationList { conversations, .. } => {
            registry.replace_from_summaries(conversations);
        }
        AppServerNotification::ConversationHistory {
            conversation_id,
            turns,
        } => {
            registry.update_from_history(conversation_id, turns);
        }
        AppServerNotification::ConversationSwitched { conversation_id } => {
            registry.touch(conversation_id);
        }
        _ => {}
    }
}

async fn sync_execution_registry_from_notification(
    runtime: &NodeRuntime,
    notification: &AppServerNotification,
) {
    if let AppServerNotification::ConversationViewChanged {
        conversation_id,
        snapshot,
    } = notification
    {
        runtime
            .executions()
            .lock()
            .await
            .update_conversation_view(conversation_id, &snapshot.status);
    }
}

pub(crate) async fn write_app_server_message<W>(
    writer: &mut W,
    message: AppServerMessage,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let envelope = AppServerMessageEnvelope {
        message,
        event_seq: None,
    };
    let payload = serde_json::to_string(&JsonRpcMessage::from(envelope))?;
    writer.write_all(payload.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

pub(crate) async fn write_jsonrpc_response<W>(
    writer: &mut W,
    request_id: RequestId,
    result: serde_json::Value,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_string(&JsonRpcMessage::Response(JsonRpcResponse {
        id: request_id,
        result,
    }))?;
    writer.write_all(payload.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

pub(crate) async fn write_jsonrpc_error<W>(
    writer: &mut W,
    request_id: RequestId,
    error: JsonRpcErrorPayload,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_string(&JsonRpcMessage::Error(JsonRpcError {
        id: request_id,
        error,
    }))?;
    writer.write_all(payload.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
#[path = "message_sync_tests.rs"]
mod message_sync_tests;
