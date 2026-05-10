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
mod tests {
    use super::sync_registry_from_message;
    use crate::node::platform_manager::PlatformManager;
    use crate::node::runtime::NodeRuntime;
    use crate::node::worker_manager::WorkerManager;
    use agent_core::conversation::ConversationSummary;
    use agent_protocol::{AppServerMessage, AppServerNotification};
    use std::ffi::OsString;

    #[test]
    fn worker_conversation_list_replaces_shared_registry_state() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let root = std::env::temp_dir().join(format!(
                "cloudagent-gatewayd-platform-tests-{}",
                std::process::id()
            ));
            let runtime = NodeRuntime::new(
                WorkerManager::new(OsString::from("agentd.exe"), None),
                PlatformManager::load(Some(root.as_os_str()))
                    .await
                    .expect("platform manager"),
                "127.0.0.1:47070",
            );
            runtime.conversations().lock().await.touch("stale");

            sync_registry_from_message(
                &runtime,
                &AppServerMessage::Notification(AppServerNotification::ConversationList {
                    conversation_id: "conversation-1".to_string(),
                    conversations: vec![ConversationSummary {
                        conversation_id: "conversation-1".to_string(),
                        title: Some("Alpha".to_string()),
                        message_count: 4,
                        updated_at_ms: 12,
                    }],
                }),
            )
            .await;

            let summaries = runtime.conversations().lock().await.summaries();
            assert_eq!(summaries.len(), 1);
            assert_eq!(summaries[0].conversation_id, "conversation-1");
            assert_eq!(summaries[0].title.as_deref(), Some("Alpha"));
            assert_eq!(summaries[0].message_count, 4);
        });
    }
}
