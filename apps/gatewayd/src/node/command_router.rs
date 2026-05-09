use crate::node::message_sync::{
    write_app_server_message, write_jsonrpc_error, write_jsonrpc_response,
};
use crate::node::runtime::NodeRuntime;
use crate::node::session_state::NodeSessionState;
use crate::node::worker_manager::NodeEvent;
use agent_core::conversation::ConversationSummary;
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerNotification,
    ConversationHistoryPageResponse, ConversationHistoryResponse, ConversationListResponse,
    ConversationStatusResponse, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcRequest, RequestId,
};
use anyhow::{Context, Result};
use tokio::io::AsyncWrite;
use tokio::sync::broadcast;
use tokio::time::{Duration, timeout};

const WORKER_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn handle_command_message<W>(
    rpc: JsonRpcMessage,
    runtime: &NodeRuntime,
    session: &mut NodeSessionState,
    writer: &mut W,
) -> Result<bool>
where
    W: AsyncWrite + Unpin,
{
    let envelope = match AppClientCommandEnvelope::try_from(rpc.clone()) {
        Ok(envelope) => envelope,
        Err(error) => {
            if let JsonRpcMessage::Request(JsonRpcRequest { id, .. }) = rpc {
                write_jsonrpc_error(
                    writer,
                    id,
                    classify_command_error_for_request(error.to_string()),
                )
                .await?;
                return Ok(true);
            }
            return Err(error).context("failed to decode local node command");
        }
    };
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
    let response_subscription = runtime.workers().subscribe(&target_conversation).await?;
    *session.active_subscription_mut() =
        Some(runtime.workers().subscribe(&target_conversation).await?);
    runtime
        .workers()
        .send_command(&target_conversation, envelope.command.clone())
        .await?;
    maybe_write_worker_backed_response(
        writer,
        &envelope.command,
        &envelope.request_id,
        &target_conversation,
        response_subscription,
    )
    .await?;
    Ok(true)
}

fn classify_command_error_for_request(message: String) -> JsonRpcErrorPayload {
    let code = if message.contains("unsupported request method") {
        -32601
    } else {
        -32602
    };
    JsonRpcErrorPayload {
        code,
        message,
        data: None,
    }
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

async fn maybe_write_worker_backed_response<W>(
    writer: &mut W,
    command: &AppClientCommand,
    request_id: &RequestId,
    conversation_id: &str,
    mut events: broadcast::Receiver<NodeEvent>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    if !matches!(
        command,
        AppClientCommand::RequestConversationStatus { .. }
            | AppClientCommand::RequestConversationHistory { .. }
            | AppClientCommand::RequestConversationHistoryPage { .. }
    ) {
        return Ok(());
    }

    let result = timeout(WORKER_RESPONSE_TIMEOUT, async {
        loop {
            match events.recv().await {
                Ok(NodeEvent::Message { message }) => {
                    if let Some(response) =
                        worker_response_value(command, conversation_id, message.as_ref())?
                    {
                        return Ok(response);
                    }
                }
                Ok(NodeEvent::Diagnostic { message, .. }) => {
                    return Err(anyhow::anyhow!(message));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(anyhow::anyhow!("worker response stream closed"));
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    return Err(anyhow::anyhow!(
                        "worker response stream lagged; skipped {skipped} events"
                    ));
                }
            }
        }
    })
    .await;

    match result {
        Ok(Ok(value)) => write_jsonrpc_response(writer, request_id.clone(), value).await,
        Ok(Err(error)) => {
            write_jsonrpc_error(
                writer,
                request_id.clone(),
                JsonRpcErrorPayload {
                    code: -32000,
                    message: error.to_string(),
                    data: None,
                },
            )
            .await
        }
        Err(_) => {
            write_jsonrpc_error(
                writer,
                request_id.clone(),
                JsonRpcErrorPayload {
                    code: -32000,
                    message: format!(
                        "timed out waiting for worker response to `{}`",
                        request_method_name(command)
                    ),
                    data: None,
                },
            )
            .await
        }
    }
}

fn worker_response_value(
    command: &AppClientCommand,
    conversation_id: &str,
    message: &AppServerMessage,
) -> Result<Option<serde_json::Value>> {
    let AppServerMessage::Notification(notification) = message else {
        return Ok(None);
    };

    let value = match (command, notification) {
        (
            AppClientCommand::RequestConversationStatus { .. },
            AppServerNotification::ConversationStatus {
                conversation_id: response_conversation_id,
                snapshot,
            },
        ) if response_conversation_id == conversation_id => {
            serde_json::to_value(ConversationStatusResponse {
                snapshot: snapshot.clone(),
            })?
        }
        (
            AppClientCommand::RequestConversationHistory { .. },
            AppServerNotification::ConversationHistory {
                conversation_id: response_conversation_id,
                turns,
            },
        ) if response_conversation_id == conversation_id => {
            serde_json::to_value(ConversationHistoryResponse {
                turns: turns.clone(),
            })?
        }
        (
            AppClientCommand::RequestConversationHistoryPage { .. },
            AppServerNotification::ConversationHistoryPage {
                conversation_id: response_conversation_id,
                turns,
                has_more,
                next_before_turn_id,
            },
        ) if response_conversation_id == conversation_id => {
            serde_json::to_value(ConversationHistoryPageResponse {
                turns: turns.clone(),
                has_more: *has_more,
                next_before_turn_id: next_before_turn_id.clone(),
            })?
        }
        _ => return Ok(None),
    };

    Ok(Some(value))
}

fn request_method_name(command: &AppClientCommand) -> &'static str {
    match command {
        AppClientCommand::RequestConversationStatus { .. } => "conversation/status",
        AppClientCommand::RequestConversationHistory { .. } => "conversation/history",
        AppClientCommand::RequestConversationHistoryPage { .. } => "conversation/historyPage",
        AppClientCommand::ListConversations => "conversation/list",
        _ => "unknown",
    }
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
    use super::{conversation_list_response, target_conversation_id, worker_response_value};
    use crate::node::runtime::NodeRuntime;
    use crate::node::session_state::NodeSessionState;
    use crate::node::worker_manager::WorkerManager;
    use agent_core::conversation::{ConversationSnapshot, ConversationStatus};
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

    #[test]
    fn history_request_maps_notification_to_typed_response_value() {
        let turns = vec![];
        let value = worker_response_value(
            &AppClientCommand::RequestConversationHistory {
                conversation_id: "conversation-1".to_string(),
            },
            "conversation-1",
            &AppServerMessage::Notification(AppServerNotification::ConversationHistory {
                conversation_id: "conversation-1".to_string(),
                turns: turns.clone(),
            }),
        )
        .expect("response mapping")
        .expect("typed response value");

        let response: agent_protocol::ConversationHistoryResponse =
            serde_json::from_value(value).expect("decode history response");
        assert_eq!(response.turns.len(), turns.len());
    }

    #[test]
    fn status_request_maps_notification_to_typed_response_value() {
        let snapshot = ConversationSnapshot {
            conversation_id: "conversation-1".to_string(),
            conversation_status: ConversationStatus::Idle,
            active_turn: None,
            turn_state: None,
            message_count: 3,
        };
        let value = worker_response_value(
            &AppClientCommand::RequestConversationStatus {
                conversation_id: "conversation-1".to_string(),
            },
            "conversation-1",
            &AppServerMessage::Notification(AppServerNotification::ConversationStatus {
                conversation_id: "conversation-1".to_string(),
                snapshot: snapshot.clone(),
            }),
        )
        .expect("response mapping")
        .expect("typed response value");

        let response: agent_protocol::ConversationStatusResponse =
            serde_json::from_value(value).expect("decode status response");
        assert_eq!(response.snapshot.conversation_id, snapshot.conversation_id);
        assert!(matches!(
            response.snapshot.conversation_status,
            ConversationStatus::Idle
        ));
        assert_eq!(response.snapshot.message_count, 3);
    }
}
