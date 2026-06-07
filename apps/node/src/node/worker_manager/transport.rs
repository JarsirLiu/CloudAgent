use agent_app_server_client::{StdioAppServerClient, TypedRequestError};
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, CommandExecutionContext, JsonRpcMessage,
    JsonRpcRequest,
};
use std::ffi::OsString;

pub(super) const ERR_TRANSPORT_CLOSED_PREFIX: &str = "ERR_TRANSPORT_CLOSED:";

pub(super) fn worker_command_name(command: &AppClientCommand) -> &'static str {
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

pub(super) fn normalize_worker_disconnect_message(message: &str) -> String {
    match message.trim() {
        "stdio app server closed" => {
            format!("{ERR_TRANSPORT_CLOSED_PREFIX} worker app server closed unexpectedly")
        }
        other => format!("{ERR_TRANSPORT_CLOSED_PREFIX} {other}"),
    }
}

pub(super) fn worker_stdio_args(data_root_dir: Option<OsString>) -> Vec<OsString> {
    let mut args = vec![OsString::from("app-server-stdio")];
    if let Some(data_root_dir) = data_root_dir {
        args.push(OsString::from("--data-dir"));
        args.push(data_root_dir);
    }
    args
}

pub(super) async fn request_worker_json(
    client: &StdioAppServerClient,
    request: JsonRpcRequest,
    context: Option<CommandExecutionContext>,
) -> Result<serde_json::Value, TypedRequestError> {
    if let Some(context) = context {
        let envelope = AppClientCommandEnvelope::try_from(JsonRpcMessage::Request(request))
            .map_err(|error| TypedRequestError::Transport {
                method: "worker/request".to_string(),
                source: std::io::Error::other(error.to_string()),
            })?;
        let request = match JsonRpcMessage::from(AppClientCommandEnvelope {
            request_id: envelope.request_id,
            command: envelope.command,
            context: Some(context),
        }) {
            JsonRpcMessage::Request(request) => request,
            _ => unreachable!("command envelope must serialize as request"),
        };
        client.request_typed::<serde_json::Value>(request).await
    } else {
        client.request_typed::<serde_json::Value>(request).await
    }
}
