use crate::node::message_sync::{
    sync_registry_from_message, write_app_server_message, write_jsonrpc_error,
    write_jsonrpc_response,
};
use crate::node::runtime::NodeRuntime;
use crate::node::session_state::NodeSessionState;
use agent_core::conversation::ConversationSummary;
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerNotification,
    ConversationHistoryPageResponse, ConversationHistoryResponse, ConversationListResponse,
    ConversationStatusResponse, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcRequest, RequestId,
};
use anyhow::{Context, Result};
use tokio::io::AsyncWrite;

pub(crate) async fn handle_command_message<W>(
    rpc: JsonRpcMessage,
    runtime: &NodeRuntime,
    session: &mut NodeSessionState,
    writer: &mut W,
) -> Result<bool>
where
    W: AsyncWrite + Unpin,
{
    if let Some(should_continue) =
        handle_typed_request_message(&rpc, runtime, session, writer).await?
    {
        return Ok(should_continue);
    }

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

    if let Some(error) = hub_mode_only_error_response(&rpc, &envelope.command) {
        match error {
            HubModeOnlyResponse::JsonRpc { request_id, error } => {
                write_jsonrpc_error(writer, request_id, error).await?;
            }
            HubModeOnlyResponse::Notification(message) => {
                write_app_server_message(writer, *message).await?;
            }
        }
        return Ok(true);
    }

    if let Some(message) = conversation_list_notification(
        &rpc,
        &envelope.command,
        session.active_conversation_id(),
        runtime,
    )
    .await
    {
        write_app_server_message(writer, message).await?;
        return Ok(true);
    }

    let target_conversation = target_conversation_id(session, runtime, &envelope.command).await;
    ensure_session_subscription(runtime, session, &target_conversation).await?;
    if let Some(request) = typed_request_from_command(&envelope.command, &envelope.request_id) {
        match envelope.command {
            AppClientCommand::RequestConversationStatus { .. } => {
                let value = runtime
                    .workers()
                    .request_json(&target_conversation, request)
                    .await
                    .map_err(anyhow::Error::from)?;
                let response: ConversationStatusResponse = serde_json::from_value(value)?;
                write_jsonrpc_response(
                    writer,
                    envelope.request_id,
                    serde_json::to_value(response)?,
                )
                .await?;
            }
            AppClientCommand::RequestConversationHistory { .. } => {
                let value = runtime
                    .workers()
                    .request_json(&target_conversation, request)
                    .await
                    .map_err(anyhow::Error::from)?;
                let response: ConversationHistoryResponse = serde_json::from_value(value)?;
                write_jsonrpc_response(
                    writer,
                    envelope.request_id,
                    serde_json::to_value(response)?,
                )
                .await?;
            }
            AppClientCommand::RequestConversationHistoryPage { .. } => {
                let value = runtime
                    .workers()
                    .request_json(&target_conversation, request)
                    .await
                    .map_err(anyhow::Error::from)?;
                let response: ConversationHistoryPageResponse = serde_json::from_value(value)?;
                write_jsonrpc_response(
                    writer,
                    envelope.request_id,
                    serde_json::to_value(response)?,
                )
                .await?;
            }
            _ => unreachable!("typed_request_from_command only returns typed worker requests"),
        }
    } else {
        runtime
            .workers()
            .send_command(&target_conversation, envelope.command.clone())
            .await?;
    }
    Ok(true)
}

async fn handle_typed_request_message<W>(
    rpc: &JsonRpcMessage,
    runtime: &NodeRuntime,
    session: &mut NodeSessionState,
    writer: &mut W,
) -> Result<Option<bool>>
where
    W: AsyncWrite + Unpin,
{
    let JsonRpcMessage::Request(request) = rpc else {
        return Ok(None);
    };
    let envelope = match AppClientCommandEnvelope::try_from(rpc.clone()) {
        Ok(envelope) => envelope,
        Err(error) => {
            write_jsonrpc_error(
                writer,
                request.id.clone(),
                classify_command_error_for_request(error.to_string()),
            )
            .await?;
            return Ok(Some(true));
        }
    };

    if let Some(error) = hub_mode_only_error_response(rpc, &envelope.command) {
        match error {
            HubModeOnlyResponse::JsonRpc { request_id, error } => {
                write_jsonrpc_error(writer, request_id, error).await?;
            }
            HubModeOnlyResponse::Notification(_) => {
                unreachable!("request path always returns jsonrpc")
            }
        }
        return Ok(Some(true));
    }

    let Some(typed_request) = typed_request_from_command(&envelope.command, &envelope.request_id)
    else {
        return Ok(None);
    };

    let target_conversation = target_conversation_id(session, runtime, &envelope.command).await;
    ensure_session_subscription(runtime, session, &target_conversation).await?;

    let result = match &envelope.command {
        AppClientCommand::ListConversations => {
            serde_json::to_value(read_conversation_list(runtime, &target_conversation).await?)?
        }
        _ => {
            let value = runtime
                .workers()
                .request_json(&target_conversation, typed_request)
                .await
                .map_err(anyhow::Error::from)?;
            sync_typed_read_registry(runtime, &envelope.command, &target_conversation, &value)
                .await?;
            value
        }
    };

    write_jsonrpc_response(writer, envelope.request_id, result).await?;
    Ok(Some(true))
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

fn typed_request_from_command(
    command: &AppClientCommand,
    request_id: &RequestId,
) -> Option<JsonRpcRequest> {
    let (method, params) = match command {
        AppClientCommand::ListConversations => ("conversation/list", serde_json::Value::Null),
        AppClientCommand::RequestConversationStatus { conversation_id } => (
            "conversation/status",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationHistory { conversation_id } => (
            "conversation/history",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationHistoryPage {
            conversation_id,
            before_turn_id,
            limit,
        } => (
            "conversation/historyPage",
            serde_json::json!({
                "conversation_id": conversation_id,
                "before_turn_id": before_turn_id,
                "limit": limit,
            }),
        ),
        _ => return None,
    };
    Some(JsonRpcRequest {
        id: request_id.clone(),
        method: method.to_string(),
        params: Some(params),
    })
}

#[derive(Debug)]
enum HubModeOnlyResponse {
    JsonRpc {
        request_id: RequestId,
        error: JsonRpcErrorPayload,
    },
    Notification(Box<AppServerMessage>),
}

fn hub_mode_only_error_response(
    rpc: &JsonRpcMessage,
    command: &AppClientCommand,
) -> Option<HubModeOnlyResponse> {
    let unsupported = match command {
        AppClientCommand::ListOnlineNodes => "ListOnlineNodes",
        AppClientCommand::SelectTargetNode { .. } => "SelectTargetNode",
        _ => return None,
    };
    let message =
        format!("hub mode only: `{unsupported}` is not available for the current direct target");
    match rpc {
        JsonRpcMessage::Request(JsonRpcRequest { id, .. }) => Some(HubModeOnlyResponse::JsonRpc {
            request_id: id.clone(),
            error: JsonRpcErrorPayload {
                code: -32000,
                message,
                data: None,
            },
        }),
        JsonRpcMessage::Notification(_) => Some(HubModeOnlyResponse::Notification(Box::new(
            AppServerMessage::Notification(AppServerNotification::Error {
                conversation_id: "default".to_string(),
                message,
            }),
        ))),
        JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => None,
    }
}

async fn conversation_list_notification(
    rpc: &JsonRpcMessage,
    command: &AppClientCommand,
    active_conversation_id: &str,
    runtime: &NodeRuntime,
) -> Option<AppServerMessage> {
    if !matches!(rpc, JsonRpcMessage::Notification(_))
        || !matches!(command, AppClientCommand::ListConversations)
    {
        return None;
    }
    Some(AppServerMessage::Notification(
        AppServerNotification::ConversationList {
            conversation_id: active_conversation_id.to_string(),
            conversations: read_conversation_list(runtime, active_conversation_id)
                .await
                .ok()?
                .conversations,
        },
    ))
}

async fn read_conversation_list(
    runtime: &NodeRuntime,
    active_conversation_id: &str,
) -> Result<ConversationListResponse> {
    let summaries: Vec<ConversationSummary> = runtime.conversations().lock().await.summaries();
    if !summaries.is_empty() {
        return Ok(ConversationListResponse {
            conversations: summaries,
        });
    }

    let value = runtime
        .workers()
        .request_json(
            active_conversation_id,
            JsonRpcRequest {
                id: RequestId::String("conversation-list".to_string()),
                method: "conversation/list".to_string(),
                params: None,
            },
        )
        .await
        .map_err(anyhow::Error::from)?;
    let response: ConversationListResponse = serde_json::from_value(value)?;
    Ok(response)
}

async fn sync_typed_read_registry(
    runtime: &NodeRuntime,
    command: &AppClientCommand,
    conversation_id: &str,
    value: &serde_json::Value,
) -> Result<()> {
    let message = match command {
        AppClientCommand::RequestConversationHistory { .. } => {
            let response: ConversationHistoryResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::ConversationHistory {
                conversation_id: conversation_id.to_string(),
                turns: response.turns,
            })
        }
        AppClientCommand::RequestConversationStatus { .. } => {
            let response: ConversationStatusResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::ConversationStatus {
                conversation_id: conversation_id.to_string(),
                snapshot: response.snapshot,
            })
        }
        AppClientCommand::ListConversations => {
            let response: ConversationListResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::ConversationList {
                conversation_id: conversation_id.to_string(),
                conversations: response.conversations,
            })
        }
        AppClientCommand::RequestConversationHistoryPage { .. } => return Ok(()),
        _ => return Ok(()),
    };
    sync_registry_from_message(runtime, &message).await;
    Ok(())
}

async fn ensure_session_subscription(
    runtime: &NodeRuntime,
    session: &mut NodeSessionState,
    target_conversation: &str,
) -> Result<()> {
    *session.active_subscription_mut() =
        Some(runtime.workers().subscribe(target_conversation).await?);
    Ok(())
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
        | AppClientCommand::SubscribeConversation { conversation_id } => {
            let mut registry = runtime.conversations().lock().await;
            registry.touch(conversation_id);
            if let AppClientCommand::SetConversationTitle { title, .. } = command {
                registry.set_title(conversation_id, title.clone());
            }
            session.set_active_conversation_id(conversation_id.clone());
            session.subscribe_conversation(conversation_id.clone());
            conversation_id.clone()
        }
        AppClientCommand::UnsubscribeConversation { conversation_id } => {
            session.unsubscribe_conversation(conversation_id);
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
    use super::{
        conversation_list_notification, ensure_session_subscription, read_conversation_list,
        target_conversation_id,
    };
    use crate::node::runtime::NodeRuntime;
    use crate::node::session_state::NodeSessionState;
    use crate::node::worker_manager::{NodeEvent, WorkerManager};
    use agent_protocol::JsonRpcNotification;
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, JsonRpcMessage, JsonRpcRequest,
        RequestId,
    };
    use std::ffi::OsString;
    use std::path::PathBuf;
    use tokio::sync::broadcast;

    fn test_worker_manager() -> WorkerManager {
        let root = std::env::temp_dir().join(format!(
            "cloudagent-gatewayd-tests-{}",
            std::process::id()
        ));
        WorkerManager::new(
            OsString::from("agentd.exe"),
            Some(PathBuf::from(root).into_os_string()),
        )
    }

    #[test]
    fn list_conversations_routes_to_active_conversation() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = NodeRuntime::new(test_worker_manager());
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
            let runtime = NodeRuntime::new(test_worker_manager());
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
            let runtime = NodeRuntime::new(test_worker_manager());
            {
                let mut registry = runtime.conversations().lock().await;
                registry.touch("conversation-1");
                registry.set_title("conversation-1", "Alpha".to_string());
            }

            let message = conversation_list_notification(
                &JsonRpcMessage::Notification(JsonRpcNotification {
                    method: "conversation/list".to_string(),
                    params: None,
                }),
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
    fn empty_registry_still_returns_a_typed_conversation_list_result() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = NodeRuntime::new(test_worker_manager());
            let response = read_conversation_list(&runtime, "conversation-1").await;
            assert!(response.is_ok());
        });
    }

    #[test]
    fn hub_mode_only_commands_fail_explicitly_in_direct_mode() {
        let message = super::hub_mode_only_error_response(
            &JsonRpcMessage::Notification(agent_protocol::JsonRpcNotification {
                method: "hub/node/list".to_string(),
                params: None,
            }),
            &AppClientCommand::ListOnlineNodes,
        )
        .expect("error response");
        match message {
            super::HubModeOnlyResponse::Notification(message) => {
                let AppServerMessage::Notification(AppServerNotification::Error {
                    message, ..
                }) = *message
                else {
                    panic!("unexpected notification payload");
                };
                assert!(message.contains("hub mode only"));
                assert!(message.contains("ListOnlineNodes"));
            }
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn hub_mode_only_requests_return_jsonrpc_error() {
        let response = super::hub_mode_only_error_response(
            &JsonRpcMessage::Request(JsonRpcRequest {
                id: RequestId::Integer(9),
                method: "hub/node/list".to_string(),
                params: None,
            }),
            &AppClientCommand::ListOnlineNodes,
        )
        .expect("error response");
        match response {
            super::HubModeOnlyResponse::JsonRpc { request_id, error } => {
                assert_eq!(request_id, RequestId::Integer(9));
                assert_eq!(error.code, -32000);
                assert!(error.message.contains("hub mode only"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn typed_request_from_list_conversations_uses_jsonrpc_list_method() {
        let request = super::typed_request_from_command(
            &AppClientCommand::ListConversations,
            &RequestId::Integer(5),
        )
        .expect("typed request");
        assert_eq!(request.id, RequestId::Integer(5));
        assert_eq!(request.method, "conversation/list");
        assert!(request.params.is_some());
    }

    #[tokio::test]
    async fn ensure_session_subscription_replaces_stale_receiver() {
        let runtime = NodeRuntime::new(test_worker_manager());
        let mut session = NodeSessionState::new("conversation-1");
        let (_, stale_rx) = broadcast::channel::<NodeEvent>(1);
        *session.active_subscription_mut() = Some(stale_rx);

        ensure_session_subscription(&runtime, &mut session, "conversation-1")
            .await
            .expect("subscribe shared worker");

        assert!(session.active_subscription_mut().is_some());
    }
}
