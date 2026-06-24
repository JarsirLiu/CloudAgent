use crate::gateway_event::OutboundTarget;
use crate::message::InboundMessage;
use agent_core::ServerRequestDecision;
use agent_app_server_client::AppServerEvent;
use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest};

pub(crate) fn event_conversation_id(event: &AppServerEvent) -> Option<&str> {
    match event {
        AppServerEvent::Message(message) => message.conversation_id(),
        AppServerEvent::Lagged { .. } | AppServerEvent::Disconnected { .. } => None,
    }
}

pub(crate) fn event_turn_id(event: &AppServerEvent) -> Option<&str> {
    match event {
        AppServerEvent::Message(AppServerMessage::Notification(notification)) => {
            notification_turn_id(notification)
        }
        AppServerEvent::Message(AppServerMessage::Request(_)) => None,
        AppServerEvent::Lagged { .. } | AppServerEvent::Disconnected { .. } => None,
    }
}

pub(crate) fn notification_turn_id(notification: &AppServerNotification) -> Option<&str> {
    match notification {
        AppServerNotification::TurnStarted { turn_id, .. }
        | AppServerNotification::ItemStarted { turn_id, .. }
        | AppServerNotification::AgentMessageDelta { turn_id, .. }
        | AppServerNotification::PlanDelta { turn_id, .. }
        | AppServerNotification::ReasoningSummaryTextDelta { turn_id, .. }
        | AppServerNotification::ReasoningTextDelta { turn_id, .. }
        | AppServerNotification::CommandExecutionOutputDelta { turn_id, .. }
        | AppServerNotification::ToolOutputDelta { turn_id, .. }
        | AppServerNotification::JsonPatchDelta { turn_id, .. }
        | AppServerNotification::ItemProgress { turn_id, .. }
        | AppServerNotification::ItemMetricsUpdated { turn_id, .. }
        | AppServerNotification::TokenUsageUpdated { turn_id, .. }
        | AppServerNotification::ModelRetrying { turn_id, .. }
        | AppServerNotification::ItemCompleted { turn_id, .. }
        | AppServerNotification::TurnCompleted { turn_id, .. }
        | AppServerNotification::TurnFailed { turn_id, .. }
        | AppServerNotification::TurnCancelled { turn_id, .. } => Some(turn_id.as_str()),
        _ => None,
    }
}

pub(crate) fn render_request_prompt(request: &AppServerRequest) -> String {
    let AppServerRequest::ServerRequest { request, .. } = request;
    match request {
        agent_core::ServerRequest::CommandApproval { request } => format!(
            "工具调用需要审批: 命令执行\n原因: {}\n命令: {}",
            request.reason, request.command_preview
        ),
        agent_core::ServerRequest::FileChangeApproval { request } => format!(
            "工具调用需要审批: 文件改动\n原因: {}\n变更: {}",
            request.reason, request.change_preview
        ),
    }
}

pub(crate) fn render_request_resolution_label(request: &agent_core::ServerRequest) -> &'static str {
    match request {
        agent_core::ServerRequest::CommandApproval { .. } => "命令执行",
        agent_core::ServerRequest::FileChangeApproval { .. } => "文件改动",
    }
}

pub(crate) fn parse_approval_command(text: &str) -> Option<ServerRequestDecision> {
    match text.trim().to_ascii_lowercase().as_str() {
        "/approve" | "/allow" | "/yes" => Some(ServerRequestDecision::accept(Some(
            "approved from gateway im".to_string(),
        ))),
        "/approve-session" | "/approve session" | "/always" | "/session" => {
            Some(ServerRequestDecision::accept_for_session(Some(
                "approved for session from gateway im".to_string(),
            )))
        }
        "/deny" | "/reject" | "/no" => Some(ServerRequestDecision::decline(Some(
            "denied from gateway im".to_string(),
        ))),
        "/cancel" => Some(ServerRequestDecision::cancel(Some(
            "cancelled from gateway im".to_string(),
        ))),
        _ => None,
    }
}

pub(crate) fn build_outbound_target(
    conversation_id: String,
    chat_id: String,
    chat_type: Option<String>,
    reply_context: Option<crate::message::ReplyContext>,
    is_reply_chain: bool,
) -> OutboundTarget {
    OutboundTarget {
        conversation_id,
        chat_id,
        chat_type,
        is_reply_chain,
        reply_context,
    }
}

pub(crate) fn build_turn_content(message: &InboundMessage) -> Vec<agent_core::InputItem> {
    let mut content = if message.text.is_empty() {
        Vec::new()
    } else {
        agent_core::text_input_items(message.text.clone())
    };
    for (index, path) in message.image_paths.iter().enumerate() {
        content.push(agent_core::InputItem::Image {
            source: agent_core::AttachmentRef::LocalPath { path: path.clone() },
            detail: Some(agent_core::ImageDetail::High),
            alt: Some(format!("gateway image {}", index + 1)),
        });
    }
    content
}

pub(crate) fn event_name(event: &AppServerEvent) -> &'static str {
    match event {
        AppServerEvent::Message(AppServerMessage::Notification(notification)) => {
            match notification {
                AppServerNotification::TurnStarted { .. } => "turn_started",
                AppServerNotification::ItemStarted { .. } => "item_started",
                AppServerNotification::AgentMessageDelta { .. } => "agent_message_delta",
                AppServerNotification::PlanDelta { .. } => "plan_delta",
                AppServerNotification::ReasoningSummaryTextDelta { .. } => {
                    "reasoning_summary_delta"
                }
                AppServerNotification::ReasoningTextDelta { .. } => "reasoning_text_delta",
                AppServerNotification::CommandExecutionOutputDelta { .. } => {
                    "command_output_delta"
                }
                AppServerNotification::ToolOutputDelta { .. } => "tool_output_delta",
                AppServerNotification::JsonPatchDelta { .. } => "json_patch_delta",
                AppServerNotification::ItemCompleted { item, .. } => match item.kind {
                    agent_core::TurnItemKind::AssistantMessage => "agent_message_completed",
                    agent_core::TurnItemKind::Reasoning => "reasoning_completed",
                    agent_core::TurnItemKind::CommandExecution => "command_completed",
                    agent_core::TurnItemKind::FileChange => "file_change_completed",
                    agent_core::TurnItemKind::ToolCall | agent_core::TurnItemKind::ToolResult => {
                        "tool_result_completed"
                    }
                    _ => "item_completed_other",
                },
                AppServerNotification::TurnCompleted { .. } => "turn_completed",
                AppServerNotification::TurnFailed { .. } => "turn_failed",
                AppServerNotification::TurnCancelled { .. } => "turn_cancelled",
                AppServerNotification::Info { .. } => "info",
                AppServerNotification::Error { .. } => "error",
                _ => "notification_other",
            }
        }
        AppServerEvent::Message(AppServerMessage::Request(_)) => "request",
        AppServerEvent::Lagged { .. } => "lagged",
        AppServerEvent::Disconnected { .. } => "disconnected",
    }
}
