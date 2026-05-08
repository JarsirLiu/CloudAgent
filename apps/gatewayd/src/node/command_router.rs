use crate::node::conversation_registry::ConversationRegistry;
use crate::node::message_sync::write_app_server_message;
use crate::node::worker_manager::{NodeEvent, WorkerManager};
use agent_core::conversation::ConversationSummary;
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerNotification,
    JsonRpcMessage,
};
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::AsyncWrite;
use tokio::sync::{Mutex, broadcast};

pub(crate) async fn handle_command_line<W>(
    line: &str,
    active_conversation_id: &mut String,
    workers: &WorkerManager,
    conversations: &Arc<Mutex<ConversationRegistry>>,
    writer: &mut W,
    active_subscription: &mut Option<broadcast::Receiver<NodeEvent>>,
) -> Result<bool>
where
    W: AsyncWrite + Unpin,
{
    let rpc: JsonRpcMessage =
        serde_json::from_str(line).context("failed to parse local node jsonrpc command")?;
    let envelope = AppClientCommandEnvelope::try_from(rpc)?;
    if matches!(envelope.command, AppClientCommand::Exit) {
        return Ok(false);
    }

    if let Some(message) =
        conversation_list_response(&envelope.command, active_conversation_id, conversations).await
    {
        write_app_server_message(writer, message).await?;
        return Ok(true);
    }

    let target_conversation =
        target_conversation_id(active_conversation_id, conversations, &envelope.command).await;
    *active_subscription = Some(workers.subscribe(&target_conversation).await?);
    workers
        .send_command(&target_conversation, envelope.command)
        .await?;
    Ok(true)
}

async fn conversation_list_response(
    command: &AppClientCommand,
    active_conversation_id: &str,
    conversations: &Arc<Mutex<ConversationRegistry>>,
) -> Option<AppServerMessage> {
    if !matches!(command, AppClientCommand::ListConversations) {
        return None;
    }
    let summaries: Vec<ConversationSummary> = conversations.lock().await.summaries();
    Some(AppServerMessage::Notification(
        AppServerNotification::ConversationList {
            conversation_id: active_conversation_id.to_string(),
            conversations: summaries,
        },
    ))
}

pub(crate) async fn target_conversation_id(
    active_conversation_id: &mut String,
    conversations: &Arc<Mutex<ConversationRegistry>>,
    command: &AppClientCommand,
) -> String {
    match command {
        AppClientCommand::SwitchConversation { conversation_id }
        | AppClientCommand::CreateConversation { conversation_id }
        | AppClientCommand::SubmitTurn(agent_protocol::UserTurnInput {
            conversation_id, ..
        })
        | AppClientCommand::ResolveServerRequest {
            conversation_id, ..
        }
        | AppClientCommand::InterruptTurn { conversation_id }
        | AppClientCommand::CompactConversation { conversation_id }
        | AppClientCommand::ResetConversation { conversation_id }
        | AppClientCommand::RequestConversationStatus { conversation_id }
        | AppClientCommand::RequestConversationHistory { conversation_id }
        | AppClientCommand::RequestConversationHistoryPage {
            conversation_id, ..
        }
        | AppClientCommand::SetConversationTitle {
            conversation_id, ..
        }
        | AppClientCommand::ArchiveConversation { conversation_id }
        | AppClientCommand::DeleteConversation { conversation_id }
        | AppClientCommand::SubscribeConversation { conversation_id }
        | AppClientCommand::UnsubscribeConversation { conversation_id } => {
            let mut registry = conversations.lock().await;
            registry.touch(conversation_id);
            if let AppClientCommand::SetConversationTitle { title, .. } = command {
                registry.set_title(conversation_id, title.clone());
            }
            *active_conversation_id = conversation_id.clone();
            conversation_id.clone()
        }
        AppClientCommand::ListConversations | AppClientCommand::Exit => {
            active_conversation_id.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{conversation_list_response, target_conversation_id};
    use crate::node::conversation_registry::ConversationRegistry;
    use agent_protocol::{AppClientCommand, AppServerMessage, AppServerNotification};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn list_conversations_routes_to_active_conversation() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));
            let mut active = "conversation-1".to_string();
            assert_eq!(
                target_conversation_id(
                    &mut active,
                    &conversations,
                    &AppClientCommand::ListConversations,
                )
                .await,
                "conversation-1"
            );
        });
    }

    #[test]
    fn switch_conversation_updates_active_conversation() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));
            let mut active = "conversation-1".to_string();
            let command = AppClientCommand::SwitchConversation {
                conversation_id: "conversation-2".to_string(),
            };

            assert_eq!(
                target_conversation_id(&mut active, &conversations, &command).await,
                "conversation-2"
            );
            assert_eq!(active, "conversation-2");
            assert_eq!(conversations.lock().await.summaries().len(), 1);
        });
    }

    #[test]
    fn list_conversations_uses_node_shared_registry() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));
            {
                let mut registry = conversations.lock().await;
                registry.touch("conversation-1");
                registry.set_title("conversation-1", "Alpha".to_string());
            }

            let message = conversation_list_response(
                &AppClientCommand::ListConversations,
                "conversation-1",
                &conversations,
            )
            .await
            .expect("conversation list message");

            match message {
                AppServerMessage::Notification(AppServerNotification::ConversationList {
                    conversation_id,
                    conversations,
                }) => {
                    assert_eq!(conversation_id, "conversation-1");
                    assert_eq!(conversations.len(), 1);
                    assert_eq!(conversations[0].title.as_deref(), Some("Alpha"));
                }
                other => panic!("unexpected message: {other:?}"),
            }
        });
    }
}
