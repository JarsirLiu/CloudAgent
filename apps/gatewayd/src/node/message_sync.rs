use crate::node::conversation_registry::ConversationRegistry;
use crate::node::worker_manager::NodeEvent;
use agent_protocol::{
    AppServerMessage, AppServerMessageEnvelope, AppServerNotification, JsonRpcMessage,
};
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

pub(crate) async fn write_node_event<W>(
    writer: &mut W,
    event: NodeEvent,
    conversations: &Arc<Mutex<ConversationRegistry>>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message = match event {
        NodeEvent::Message { message } => message,
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
    sync_registry_from_message(conversations, &message).await;
    write_app_server_message(writer, message).await
}

pub(crate) async fn sync_registry_from_message(
    conversations: &Arc<Mutex<ConversationRegistry>>,
    message: &AppServerMessage,
) {
    let AppServerMessage::Notification(notification) = message else {
        return;
    };

    let mut registry = conversations.lock().await;
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

#[cfg(test)]
mod tests {
    use super::sync_registry_from_message;
    use crate::node::conversation_registry::ConversationRegistry;
    use agent_core::conversation::ConversationSummary;
    use agent_protocol::{AppServerMessage, AppServerNotification};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn worker_conversation_list_replaces_shared_registry_state() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));
            conversations.lock().await.touch("stale");

            sync_registry_from_message(
                &conversations,
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

            let summaries = conversations.lock().await.summaries();
            assert_eq!(summaries.len(), 1);
            assert_eq!(summaries[0].conversation_id, "conversation-1");
            assert_eq!(summaries[0].title.as_deref(), Some("Alpha"));
            assert_eq!(summaries[0].message_count, 4);
        });
    }
}
