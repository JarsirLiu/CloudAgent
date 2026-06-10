use crate::node::message_sync::{
    sync_registry_from_message, write_app_server_message, write_jsonrpc_error,
    write_jsonrpc_response,
};
use crate::node::runtime::NodeRuntime;
use crate::node::session_state::NodeSessionState;
use agent_core::conversation::ConversationSummary;
use agent_core::conversation_busy_error;
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerNotification,
    CommandExecutionContext, ConversationHistoryPageResponse, ConversationHistoryResponse,
    ConversationListPageResponse, ConversationListResponse, ConversationViewResponse,
    JsonRpcErrorPayload, JsonRpcMessage, JsonRpcRequest, NodeStatusResponse, NodeStopResponse,
    PlatformConfigResponse, PlatformControlListResponse, PlatformControlStatusResponse,
    PlatformControlUpdateResponse, RequestId, SkillsListResponse, WeixinLoginStartResponse,
    WeixinLoginStatusResponse,
};
use anyhow::{Context, Result};
use tokio::io::AsyncWrite;
use tracing::info;

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
    session.apply_command_context(envelope.context.as_ref());

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
    if !matches!(
        envelope.command,
        AppClientCommand::SubscribeConversation { .. }
    ) {
        info!(
            source_domain = %session.source_domain_id(),
            command = %command_name(&envelope.command),
            target_conversation = %target_conversation,
            "node.command.received"
        );
    }
    if let Some(rejection) =
        conversation_busy_rejection(&rpc, runtime, &envelope.command, &target_conversation).await
    {
        match rejection {
            CommandRejection::JsonRpc { request_id, error } => {
                write_jsonrpc_error(writer, request_id, error).await?;
            }
            CommandRejection::Notification(message) => {
                write_app_server_message(writer, *message).await?;
            }
        }
        return Ok(true);
    }
    if command_requires_worker(&envelope.command) {
        ensure_session_subscription(runtime, session, &target_conversation).await?;
    }
    if let Some(request) = typed_request_from_command(&envelope.command, &envelope.request_id) {
        match envelope.command {
            AppClientCommand::RequestConversationView { .. } => {
                let value = runtime
                    .workers()
                    .request_json(
                        session.worker_scope_key(),
                        &target_conversation,
                        request,
                        command_execution_context(session),
                    )
                    .await
                    .map_err(anyhow::Error::from)?;
                let response: ConversationViewResponse = serde_json::from_value(value)?;
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
                    .request_json(
                        session.worker_scope_key(),
                        &target_conversation,
                        request,
                        command_execution_context(session),
                    )
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
                    .request_json(
                        session.worker_scope_key(),
                        &target_conversation,
                        request,
                        command_execution_context(session),
                    )
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
            .send_command(
                session.worker_scope_key(),
                &target_conversation,
                envelope.command.clone(),
                command_execution_context(session),
            )
            .await?;
    }
    Ok(true)
}

fn command_execution_context(session: &NodeSessionState) -> Option<CommandExecutionContext> {
    let workspace_root = session
        .workspace_root()
        .map(|path| path.to_string_lossy().into_owned());
    let cwd = session
        .cwd()
        .map(|path| path.to_string_lossy().into_owned());
    let permission_mode = session.permission_mode().map(ToString::to_string);
    let session_id = session.session_id().map(ToString::to_string);
    let data_root_dir = session
        .data_root_dir()
        .map(|path| path.to_string_lossy().into_owned());

    if session_id.is_none()
        && workspace_root.is_none()
        && cwd.is_none()
        && permission_mode.is_none()
        && data_root_dir.is_none()
    {
        return None;
    }

    Some(CommandExecutionContext {
        session_id,
        workspace_id: None,
        workspace_root,
        cwd,
        permission_mode,
        data_root_dir,
    })
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
    session.apply_command_context(envelope.context.as_ref());

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

    if matches!(envelope.command, AppClientCommand::ListSkills) {
        let result = serde_json::to_value(SkillsListResponse {
            skills: runtime.list_skills(),
        })?;
        write_jsonrpc_response(writer, envelope.request_id, result).await?;
        return Ok(Some(true));
    }

    let Some(typed_request) = typed_request_from_command(&envelope.command, &envelope.request_id)
    else {
        return Ok(None);
    };

    let target_conversation = target_conversation_id(session, runtime, &envelope.command).await;
    if let Some(rejection) =
        conversation_busy_rejection(rpc, runtime, &envelope.command, &target_conversation).await
    {
        match rejection {
            CommandRejection::JsonRpc { request_id, error } => {
                write_jsonrpc_error(writer, request_id, error).await?;
            }
            CommandRejection::Notification(message) => {
                write_app_server_message(writer, *message).await?;
            }
        }
        return Ok(Some(true));
    }
    if command_requires_worker(&envelope.command) {
        ensure_session_subscription(runtime, session, &target_conversation).await?;
    }

    let result = match &envelope.command {
        AppClientCommand::ListConversations => serde_json::to_value(
            read_conversation_list(runtime, session.worker_scope_key(), &target_conversation)
                .await?,
        )?,
        AppClientCommand::ListConversationsPage { cursor, limit } => serde_json::to_value(
            read_conversation_list_page(runtime, cursor.clone(), *limit).await?,
        )?,
        AppClientCommand::ListPlatforms => serde_json::to_value(runtime.platforms().list().await)?,
        AppClientCommand::GetNodeStatus => serde_json::to_value(runtime.status().await)?,
        AppClientCommand::StopNode => {
            runtime.request_shutdown();
            serde_json::to_value(NodeStopResponse { stopping: true })?
        }
        AppClientCommand::GetPlatformStatus { platform } => {
            serde_json::to_value(runtime.platforms().status(platform).await?)?
        }
        AppClientCommand::GetPlatformConfig { platform } => {
            serde_json::to_value(runtime.platforms().config(platform).await?)?
        }
        AppClientCommand::SetPlatformEnabled { platform, enabled } => serde_json::to_value(
            runtime
                .platforms()
                .set_enabled(platform, *enabled, runtime.listen_address())
                .await?,
        )?,
        AppClientCommand::SetPlatformConfigValue {
            platform,
            key,
            value,
        } => serde_json::to_value(
            runtime
                .platforms()
                .set_config_value(platform, key, value, runtime.listen_address())
                .await?,
        )?,
        AppClientCommand::ClearPlatformConfigValue { platform, key } => serde_json::to_value(
            runtime
                .platforms()
                .clear_config_value(platform, key, runtime.listen_address())
                .await?,
        )?,
        AppClientCommand::StartWeixinLogin => {
            serde_json::to_value(runtime.platforms().start_weixin_login().await?)?
        }
        AppClientCommand::CheckWeixinLogin { session_id } => {
            serde_json::to_value(runtime.platforms().check_weixin_login(session_id).await?)?
        }
        _ => {
            let value = runtime
                .workers()
                .request_json(
                    session.worker_scope_key(),
                    &target_conversation,
                    typed_request,
                    command_execution_context(session),
                )
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
        AppClientCommand::ListConversationsPage { cursor, limit } => (
            "conversation/listPage",
            serde_json::json!({
                "cursor": cursor,
                "limit": limit,
            }),
        ),
        AppClientCommand::ListPlatforms => ("platform/list", serde_json::Value::Null),
        AppClientCommand::GetNodeStatus => ("node/status", serde_json::Value::Null),
        AppClientCommand::StopNode => ("node/stop", serde_json::Value::Null),
        AppClientCommand::RequestConversationView { conversation_id } => (
            "conversation/view",
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
        AppClientCommand::GetPlatformStatus { platform } => (
            "platform/status",
            serde_json::json!({ "platform": platform }),
        ),
        AppClientCommand::GetPlatformConfig { platform } => (
            "platform/config",
            serde_json::json!({ "platform": platform }),
        ),
        AppClientCommand::SetPlatformEnabled { platform, enabled } => (
            "platform/setEnabled",
            serde_json::json!({ "platform": platform, "enabled": enabled }),
        ),
        AppClientCommand::SetPlatformConfigValue {
            platform,
            key,
            value,
        } => (
            "platform/config/set",
            serde_json::json!({ "platform": platform, "key": key, "value": value }),
        ),
        AppClientCommand::ClearPlatformConfigValue { platform, key } => (
            "platform/config/clear",
            serde_json::json!({ "platform": platform, "key": key }),
        ),
        AppClientCommand::StartWeixinLogin => ("weixin/login/start", serde_json::Value::Null),
        AppClientCommand::CheckWeixinLogin { session_id } => (
            "weixin/login/check",
            serde_json::json!({ "session_id": session_id }),
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

#[derive(Debug)]
enum CommandRejection {
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

async fn conversation_busy_rejection(
    rpc: &JsonRpcMessage,
    runtime: &NodeRuntime,
    command: &AppClientCommand,
    conversation_id: &str,
) -> Option<CommandRejection> {
    if !requires_idle_conversation(command) || !runtime.is_conversation_busy(conversation_id).await
    {
        return None;
    }

    let error_message = conversation_busy_error();
    match rpc {
        JsonRpcMessage::Request(JsonRpcRequest { id, .. }) => Some(CommandRejection::JsonRpc {
            request_id: id.clone(),
            error: JsonRpcErrorPayload {
                code: -32010,
                message: error_message,
                data: None,
            },
        }),
        JsonRpcMessage::Notification(_) => Some(CommandRejection::Notification(Box::new(
            AppServerMessage::Notification(AppServerNotification::Error {
                conversation_id: conversation_id.to_string(),
                message: "conversation is busy; wait for the active turn to finish or interrupt it"
                    .to_string(),
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
    if !matches!(rpc, JsonRpcMessage::Notification(_)) {
        return None;
    }

    match command {
        AppClientCommand::ListConversations => Some(AppServerMessage::Notification(
            AppServerNotification::ConversationList {
                conversation_id: active_conversation_id.to_string(),
                conversations: read_conversation_list(
                    runtime,
                    active_conversation_id,
                    active_conversation_id,
                )
                .await
                .ok()?
                .conversations,
            },
        )),
        AppClientCommand::ListConversationsPage { cursor, limit } => {
            let page = read_conversation_list_page(runtime, cursor.clone(), *limit)
                .await
                .ok()?;
            Some(AppServerMessage::Notification(
                AppServerNotification::ConversationListPage {
                    conversation_id: active_conversation_id.to_string(),
                    conversations: page.conversations,
                    has_more: page.has_more,
                    next_cursor: page.next_cursor,
                },
            ))
        }
        _ => None,
    }
}

async fn read_conversation_list_page(
    runtime: &NodeRuntime,
    cursor: Option<String>,
    limit: usize,
) -> Result<ConversationListPageResponse> {
    let _ = runtime
        .conversation_store()
        .reconcile_missing_conversations(100)
        .await;
    match runtime
        .conversation_store()
        .list_conversations_page(cursor, limit)
        .await
    {
        Ok(page) => {
            runtime
                .conversations()
                .lock()
                .await
                .replace_from_summaries(&page.conversations);
            Ok(ConversationListPageResponse {
                conversations: page.conversations,
                has_more: page.has_more,
                next_cursor: page.next_cursor,
            })
        }
        Err(_) => {
            let conversations = runtime.conversations().lock().await.summaries();
            Ok(ConversationListPageResponse {
                conversations,
                has_more: false,
                next_cursor: None,
            })
        }
    }
}

async fn read_conversation_list(
    runtime: &NodeRuntime,
    _worker_scope_key: &str,
    _active_conversation_id: &str,
) -> Result<ConversationListResponse> {
    let _ = runtime
        .conversation_store()
        .reconcile_missing_conversations(100)
        .await;
    match runtime.conversation_store().list_conversations().await {
        Ok(conversations) => {
            let conversations = conversations
                .into_iter()
                .filter(|summary| !summary.archived)
                .map(|summary| ConversationSummary {
                    conversation_id: summary.conversation_id,
                    title: summary.title,
                    message_count: summary.message_count,
                    updated_at_ms: summary.updated_at_ms,
                })
                .collect::<Vec<_>>();
            runtime
                .conversations()
                .lock()
                .await
                .replace_from_summaries(&conversations);
            Ok(ConversationListResponse { conversations })
        }
        Err(_) => {
            let conversations = runtime.conversations().lock().await.summaries();
            Ok(ConversationListResponse { conversations })
        }
    }
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
        AppClientCommand::ListPlatforms => {
            let response: PlatformControlListResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: format!("platforms: {}", response.platforms.len()),
            })
        }
        AppClientCommand::GetNodeStatus => {
            let response: NodeStatusResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: format!(
                    "node listening on {} · worker {} · platform runtimes {}/{}",
                    response.listen_address,
                    if response.worker_running {
                        "running"
                    } else {
                        "idle"
                    },
                    response.platform_runtime_count,
                    response.managed_platform_count
                ),
            })
        }
        AppClientCommand::StopNode => {
            let _: NodeStopResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: "node shutdown requested".to_string(),
            })
        }
        AppClientCommand::GetPlatformStatus { .. } => {
            let response: PlatformControlStatusResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: format!(
                    "platform `{}` is {}",
                    response.platform.platform,
                    if response.platform.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ),
            })
        }
        AppClientCommand::GetPlatformConfig { .. }
        | AppClientCommand::SetPlatformConfigValue { .. }
        | AppClientCommand::ClearPlatformConfigValue { .. } => {
            let response: PlatformConfigResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: format!(
                    "platform `{}` config is {}",
                    response.platform,
                    if response.configured {
                        "configured"
                    } else {
                        "incomplete"
                    }
                ),
            })
        }
        AppClientCommand::SetPlatformEnabled { .. } => {
            let response: PlatformControlUpdateResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: format!(
                    "platform `{}` {}",
                    response.platform.platform,
                    if response.platform.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ),
            })
        }
        AppClientCommand::StartWeixinLogin => {
            let response: WeixinLoginStartResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: format!(
                    "Weixin login started: session `{}` · scan {}\nThen run `/weixin-login-check {}` after confirming on phone.",
                    response.session_id, response.qr_url, response.session_id
                ),
            })
        }
        AppClientCommand::CheckWeixinLogin { .. } => {
            let response: WeixinLoginStatusResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: conversation_id.to_string(),
                message: match response.status.as_str() {
                    "confirmed" => format!(
                        "Weixin login confirmed for `{}`.",
                        response.account_id.as_deref().unwrap_or("unknown")
                    ),
                    "pending" => format!(
                        "Weixin login `{}` is still waiting for scan confirmation.",
                        response.session_id
                    ),
                    "expired" => "Weixin QR expired. Run `/weixin-login` again.".to_string(),
                    _ => response
                        .message
                        .clone()
                        .unwrap_or_else(|| "Weixin login session not found.".to_string()),
                },
            })
        }
        AppClientCommand::RequestConversationView { .. } => {
            let response: ConversationViewResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::ConversationViewChanged {
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
        AppClientCommand::ListConversationsPage { .. } => {
            let response: ConversationListPageResponse = serde_json::from_value(value.clone())?;
            AppServerMessage::Notification(AppServerNotification::ConversationListPage {
                conversation_id: conversation_id.to_string(),
                conversations: response.conversations,
                has_more: response.has_more,
                next_cursor: response.next_cursor,
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
    *session.active_subscription_mut() = Some(
        runtime
            .workers()
            .subscribe(session.worker_scope_key(), target_conversation)
            .await?,
    );
    Ok(())
}

fn command_name(command: &AppClientCommand) -> &'static str {
    match command {
        AppClientCommand::SubmitTurn(_) => "submit_turn",
        AppClientCommand::ResolveServerRequest { .. } => "resolve_server_request",
        AppClientCommand::InterruptTurn { .. } => "interrupt_turn",
        AppClientCommand::CompactConversation { .. } => "compact_conversation",
        AppClientCommand::ResetConversation { .. } => "reset_conversation",
        AppClientCommand::RequestConversationView { .. } => "request_conversation_view",
        AppClientCommand::RequestConversationHistory { .. } => "request_conversation_history",
        AppClientCommand::RequestConversationHistoryPage { .. } => {
            "request_conversation_history_page"
        }
        AppClientCommand::ListConversations => "list_conversations",
        AppClientCommand::ListConversationsPage { .. } => "list_conversations_page",
        AppClientCommand::ListSkills => "list_skills",
        AppClientCommand::ListOnlineNodes => "list_online_nodes",
        AppClientCommand::ListPlatforms => "list_platforms",
        AppClientCommand::GetNodeStatus => "get_node_status",
        AppClientCommand::StopNode => "stop_node",
        AppClientCommand::SetConversationTitle { .. } => "set_conversation_title",
        AppClientCommand::CreateConversation { .. } => "create_conversation",
        AppClientCommand::SwitchConversation { .. } => "switch_conversation",
        AppClientCommand::SelectTargetNode { .. } => "select_target_node",
        AppClientCommand::GetPlatformStatus { .. } => "get_platform_status",
        AppClientCommand::GetPlatformConfig { .. } => "get_platform_config",
        AppClientCommand::SetPlatformEnabled { .. } => "set_platform_enabled",
        AppClientCommand::SetPlatformConfigValue { .. } => "set_platform_config_value",
        AppClientCommand::ClearPlatformConfigValue { .. } => "clear_platform_config_value",
        AppClientCommand::ReloadLlmConfig { .. } => "reload_llm_config",
        AppClientCommand::StartWeixinLogin => "start_weixin_login",
        AppClientCommand::CheckWeixinLogin { .. } => "check_weixin_login",
        AppClientCommand::ArchiveConversation { .. } => "archive_conversation",
        AppClientCommand::DeleteConversation { .. } => "delete_conversation",
        AppClientCommand::SubscribeConversation { .. } => "subscribe_conversation",
        AppClientCommand::UnsubscribeConversation { .. } => "unsubscribe_conversation",
        AppClientCommand::Exit => "exit",
    }
}

fn command_requires_worker(command: &AppClientCommand) -> bool {
    !matches!(
        command,
        AppClientCommand::ListConversationsPage { .. }
            | AppClientCommand::ListPlatforms
            | AppClientCommand::GetPlatformStatus { .. }
            | AppClientCommand::GetPlatformConfig { .. }
            | AppClientCommand::SetPlatformEnabled { .. }
            | AppClientCommand::SetPlatformConfigValue { .. }
            | AppClientCommand::ClearPlatformConfigValue { .. }
            | AppClientCommand::StartWeixinLogin
            | AppClientCommand::CheckWeixinLogin { .. }
            | AppClientCommand::GetNodeStatus
            | AppClientCommand::StopNode
    )
}

fn requires_idle_conversation(command: &AppClientCommand) -> bool {
    matches!(command, AppClientCommand::SubmitTurn(_))
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
        | AppClientCommand::RequestConversationView { conversation_id }
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
        | AppClientCommand::ListConversationsPage { .. }
        | AppClientCommand::ListSkills
        | AppClientCommand::ListOnlineNodes
        | AppClientCommand::ListPlatforms
        | AppClientCommand::GetNodeStatus
        | AppClientCommand::StopNode
        | AppClientCommand::SelectTargetNode { .. }
        | AppClientCommand::GetPlatformStatus { .. }
        | AppClientCommand::GetPlatformConfig { .. }
        | AppClientCommand::SetPlatformEnabled { .. }
        | AppClientCommand::SetPlatformConfigValue { .. }
        | AppClientCommand::ClearPlatformConfigValue { .. }
        | AppClientCommand::ReloadLlmConfig { .. }
        | AppClientCommand::StartWeixinLogin
        | AppClientCommand::CheckWeixinLogin { .. }
        | AppClientCommand::Exit => session.active_conversation_id().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        conversation_list_notification, ensure_session_subscription, handle_command_message,
        read_conversation_list, target_conversation_id,
    };
    use crate::node::platform::PlatformManager;
    use crate::node::runtime::NodeRuntime;
    use crate::node::session_state::NodeSessionState;
    use crate::node::test_support::{test_worker_program, unique_temp_path};
    use crate::node::worker_manager::{NodeEvent, WorkerManager};
    use agent_core::{ApprovalPolicy, PermissionProfile, SkillRuntime};
    use agent_core::{EventMsg, InputItem};
    use agent_protocol::JsonRpcNotification;
    use agent_protocol::{
        AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerNotification,
        ConversationViewStatus, JsonRpcError, JsonRpcMessage, JsonRpcRequest, RequestId,
        SkillsListResponse, TurnPolicy, UserTurnInput,
    };
    use tokio::io::{AsyncBufReadExt, BufReader, duplex};
    use tokio::sync::broadcast;

    fn test_worker_manager() -> WorkerManager {
        let root = unique_temp_path("cloudagent-node-tests");
        WorkerManager::new(test_worker_program(), Some(root.into_os_string()))
    }

    async fn test_runtime() -> NodeRuntime {
        let root = unique_temp_path("cloudagent-node-platform-tests");
        let platforms = PlatformManager::load(Some(root.as_os_str()))
            .await
            .expect("platform manager");
        NodeRuntime::new(
            test_worker_manager(),
            infra_store::JsonConversationStore::new(root.join("conversations")),
            platforms,
            "127.0.0.1:47070",
            root.clone(),
            SkillRuntime::new(true, Vec::new()),
            root,
        )
    }

    #[test]
    fn list_conversations_routes_to_active_conversation() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = test_runtime().await;
            let mut session = NodeSessionState::new("conversation-1", "session-1");
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
            let runtime = test_runtime().await;
            let mut session = NodeSessionState::new("conversation-1", "session-1");
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
    fn list_conversations_prefers_shared_store_over_registry_noise() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = test_runtime().await;
            {
                let mut registry = runtime.conversations().lock().await;
                registry.touch("conversation-1");
                registry.set_title("conversation-1", "Alpha".to_string());
            }
            runtime
                .conversation_store()
                .append_event(
                    "shared-conversation",
                    &EventMsg::TurnStarted {
                        turn_id: "turn-1".to_string(),
                        conversation_id: "shared-conversation".to_string(),
                        user_input: vec![InputItem::Text {
                            text: "hello".to_string(),
                        }],
                    },
                )
                .await
                .expect("append shared conversation event");

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
                    assert!(
                        conversations
                            .iter()
                            .all(|summary| summary.title.as_deref() != Some("Alpha"))
                    );
                    assert_eq!(conversations.len(), 1);
                    assert_eq!(conversations[0].conversation_id, "shared-conversation");
                }
                other => panic!("unexpected message: {other:?}"),
            }
        });
    }

    #[test]
    fn empty_registry_still_returns_a_typed_conversation_list_result() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = test_runtime().await;
            let response = read_conversation_list(&runtime, "session-1", "conversation-1").await;
            assert!(response.is_ok());
        });
    }

    #[test]
    fn list_conversations_reads_from_shared_store_across_sources() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let runtime = test_runtime().await;
            runtime
                .conversation_store()
                .append_event(
                    "im:feishu:conversation-1",
                    &EventMsg::TurnStarted {
                        turn_id: "turn-1".to_string(),
                        conversation_id: "im:feishu:conversation-1".to_string(),
                        user_input: vec![InputItem::Text {
                            text: "hello".to_string(),
                        }],
                    },
                )
                .await
                .expect("append shared conversation event");

            let response = read_conversation_list(&runtime, "local:cli", "conversation-1")
                .await
                .expect("conversation list");

            assert_eq!(response.conversations.len(), 1);
            assert_eq!(
                response.conversations[0].conversation_id,
                "im:feishu:conversation-1"
            );
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

    #[tokio::test]
    async fn list_skills_request_returns_typed_response_in_direct_mode() {
        let runtime = test_runtime().await;
        let mut session = NodeSessionState::new("conversation-1", "session-1");
        let (writer, reader) = duplex(4096);
        let mut writer = writer;
        let mut reader = BufReader::new(reader);
        let rpc = JsonRpcMessage::from(AppClientCommandEnvelope {
            request_id: RequestId::Integer(17),
            command: AppClientCommand::ListSkills,
            context: None,
        });

        let should_continue = handle_command_message(rpc, &runtime, &mut session, &mut writer)
            .await
            .expect("handle command");

        assert!(should_continue);

        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read response");
        let message: JsonRpcMessage = serde_json::from_str(&line).expect("parse jsonrpc");
        match message {
            JsonRpcMessage::Response(response) => {
                assert_eq!(response.id, RequestId::Integer(17));
                let payload: SkillsListResponse =
                    serde_json::from_value(response.result).expect("skills response");
                assert!(
                    payload
                        .skills
                        .iter()
                        .any(|skill| skill.name == "skill-creator")
                );
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
        let runtime = test_runtime().await;
        let mut session = NodeSessionState::new("conversation-1", "session-1");
        let (_, stale_rx) = broadcast::channel::<NodeEvent>(1);
        *session.active_subscription_mut() = Some(stale_rx);

        ensure_session_subscription(&runtime, &mut session, "conversation-1")
            .await
            .expect("subscribe shared worker");

        assert!(session.active_subscription_mut().is_some());
    }

    #[tokio::test]
    async fn submit_turn_request_is_rejected_when_node_marks_conversation_busy() {
        let runtime = test_runtime().await;
        runtime.executions().lock().await.update_conversation_view(
            "conversation-1",
            &ConversationViewStatus::Active {
                active_turn_id: Some("turn-1".to_string()),
                flags: Vec::new(),
            },
        );
        let mut session = NodeSessionState::new("conversation-1", "session-1");
        let (writer, reader) = duplex(4096);
        let mut writer = writer;
        let mut reader = BufReader::new(reader);
        let rpc = JsonRpcMessage::from(AppClientCommandEnvelope {
            request_id: RequestId::Integer(41),
            command: AppClientCommand::SubmitTurn(UserTurnInput {
                conversation_id: "conversation-1".to_string(),
                content: vec![],
                turn_policy: TurnPolicy {
                    permission_profile: PermissionProfile::ReadOnly,
                    approval_policy: ApprovalPolicy::OnRequest,
                },
            }),
            context: None,
        });

        let should_continue = handle_command_message(rpc, &runtime, &mut session, &mut writer)
            .await
            .expect("handle command");

        assert!(should_continue);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read response");
        let JsonRpcMessage::Error(JsonRpcError { id, error }) =
            serde_json::from_str(line.trim_end()).expect("parse jsonrpc error")
        else {
            panic!("expected jsonrpc error");
        };
        assert_eq!(id, RequestId::Integer(41));
        assert_eq!(error.code, -32010);
        assert!(error.message.starts_with("ERR_CONVERSATION_BUSY:"));
    }
}
