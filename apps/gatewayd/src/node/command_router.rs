use crate::node::message_sync::{write_app_server_message, write_jsonrpc_response};
use crate::node::runtime::NodeRuntime;
use crate::node::session_state::NodeSessionState;
use agent_core::conversation::ConversationSummary;
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerNotification,
    ConversationListResponse, JsonRpcMessage, RequestId,
};
use anyhow::{Context, Result};
use tokio::io::AsyncWrite;

pub(crate) async fn handle_command_line<W>(
    line: &str,
    runtime: &NodeRuntime,
    session: &mut NodeSessionState,
    writer: &mut W,
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
        hub_mode_only_response(&envelope.command, session.active_conversation_id())
    {
        write_app_server_message(writer, message).await?;
        return Ok(true);
    }

    if let Some(message) =
        conversation_list_response(&envelope.command, session.active_conversation_id(), runtime)
            .await
    {
        maybe_write_list_conversations_response(
            writer,
            &envelope.command,
            &envelope.request_id,
            &message,
        )
        .await?;
        write_app_server_message(writer, message).await?;
        return Ok(true);
    }

    let target_conversation = target_conversation_id(session, runtime, &envelope.command).await;
    *session.active_subscription_mut() =
        Some(runtime.workers().subscribe(&target_conversation).await?);
    runtime
        .workers()
        .send_command(&target_conversation, envelope.command)
        .await?;
    Ok(true)
}

async fn maybe_write_list_conversations_response<W>(
    writer: &mut W,
    command: &AppClientCommand,
    request_id: &RequestId,
    message: &AppServerMessage,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    if !matches!(command, AppClientCommand::ListConversations) {
        return Ok(());
    }
    let AppServerMessage::Notification(AppServerNotification::ConversationList {
        conversations,
        ..
    }) = message
    else {
        return Ok(());
    };

    write_jsonrpc_response(
        writer,
        request_id.clone(),
        serde_json::to_value(ConversationListResponse {
            conversations: conversations.clone(),
        })?,
    )
    .await
}

fn hub_mode_only_response(
    command: &AppClientCommand,
    active_conversation_id: &str,
) -> Option<AppServerMessage> {
    let unsupported = match command {
        AppClientCommand::ListOnlineNodes => "ListOnlineNodes",
        AppClientCommand::SelectTargetNode { .. } => "SelectTargetNode",
        _ => return None,
    };
    Some(AppServerMessage::Notification(
        AppServerNotification::Error {
            conversation_id: active_conversation_id.to_string(),
            message: format!(
                "hub mode only: `{unsupported}` is not available for the current direct target"
            ),
        },
    ))
}

async fn conversation_list_response(
    command: &AppClientCommand,
    active_conversation_id: &str,
    runtime: &NodeRuntime,
) -> Option<AppServerMessage> {
    if !matches!(command, AppClientCommand::ListConversations) {
        return None;
    }
    let summaries: Vec<ConversationSummary> = runtime.conversations().lock().await.summaries();
    if summaries.is_empty() {
        return None;
    }
    Some(AppServerMessage::Notification(
        AppServerNotification::ConversationList {
            conversation_id: active_conversation_id.to_string(),
            conversations: summaries,
        },
    ))
}

pub(crate) async fn target_conversation_id(
    session: &mut NodeSessionState,
    runtime: &NodeRuntime,
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
            let mut registry = runtime.conversations().lock().await;
            registry.touch(conversation_id);
            if let AppClientCommand::SetConversationTitle { title, .. } = command {
                registry.set_title(conversation_id, title.clone());
            }
            session.set_active_conversation_id(conversation_id.clone());
            conversation_id.clone()
        }
        AppClientCommand::ListConversations
        | AppClientCommand::ListOnlineNodes
        | AppClientCommand::SelectTargetNode { .. }
        | AppClientCommand::Exit => session.active_conversation_id().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{conversation_list_response, target_conversation_id};
    use crate::node::runtime::NodeRuntime;
    use crate::node::session_state::NodeSessionState;
    use crate::node::worker_manager::WorkerManager;
    use agent_protocol::{AppClientCommand, AppServerMessage, AppServerNotification};
    use std::ffi::OsString;

    #[test]
    fn list_conversations_routes_to_active_conversation() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = NodeRuntime::new(WorkerManager::new(OsString::from("agentd.exe")));
            let mut session = NodeSessionState::new("conversation-1");
            assert_eq!(
                target_conversation_id(
                    &mut session,
                    &runtime,
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
            let runtime = NodeRuntime::new(WorkerManager::new(OsString::from("agentd.exe")));
            let mut session = NodeSessionState::new("conversation-1");
            let command = AppClientCommand::SwitchConversation {
                conversation_id: "conversation-2".to_string(),
            };

            assert_eq!(
                target_conversation_id(&mut session, &runtime, &command).await,
                "conversation-2"
            );
            assert_eq!(session.active_conversation_id(), "conversation-2");
            assert_eq!(runtime.conversations().lock().await.summaries().len(), 1);
        });
    }

    #[test]
    fn list_conversations_uses_node_shared_registry() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = NodeRuntime::new(WorkerManager::new(OsString::from("agentd.exe")));
            {
                let mut registry = runtime.conversations().lock().await;
                registry.touch("conversation-1");
                registry.set_title("conversation-1", "Alpha".to_string());
            }

            let message = conversation_list_response(
                &AppClientCommand::ListConversations,
                "conversation-1",
                &runtime,
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

    #[test]
    fn empty_registry_defers_conversation_list_to_worker() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = NodeRuntime::new(WorkerManager::new(OsString::from("agentd.exe")));

            let message = conversation_list_response(
                &AppClientCommand::ListConversations,
                "conversation-1",
                &runtime,
            )
            .await;

            assert!(message.is_none());
        });
    }

    #[test]
    fn hub_mode_only_commands_fail_explicitly_in_direct_mode() {
        let message =
            super::hub_mode_only_response(&AppClientCommand::ListOnlineNodes, "conversation-1")
                .expect("error response");
        match message {
            AppServerMessage::Notification(AppServerNotification::Error {
                conversation_id,
                message,
            }) => {
                assert_eq!(conversation_id, "conversation-1");
                assert!(message.contains("hub mode only"));
                assert!(message.contains("ListOnlineNodes"));
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }
}
