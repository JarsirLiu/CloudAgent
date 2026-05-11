use crate::gateway_outbound::{
    GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate, OutboundTarget,
};
use agent_app_server_client::AppServerEvent;
use agent_core::{TranscriptItem, TurnItemKind};
use agent_protocol::{AppServerMessage, AppServerNotification};
use tracing::info;

pub enum EventFlow {
    Continue(Vec<GatewayOutbound>),
    Completed(Vec<GatewayOutbound>),
}

pub fn map_app_server_event(target: &OutboundTarget, event: AppServerEvent) -> EventFlow {
    match event {
        AppServerEvent::Message(message) => map_app_server_message(target, &message),
        AppServerEvent::Lagged { skipped } => {
            info!(
                conversation_id = %target.conversation_id,
                skipped,
                "gateway.runtime.event.lagged"
            );
            EventFlow::Continue(vec![GatewayOutbound::Info {
                target: target.clone(),
                message: format!("内部事件流暂时拥塞，跳过了 {skipped} 条低优先级事件。"),
            }])
        }
        AppServerEvent::Disconnected { message } => {
            info!(
                conversation_id = %target.conversation_id,
                message_preview = %preview(&message, 120),
                "gateway.runtime.event.disconnected"
            );
            EventFlow::Completed(vec![GatewayOutbound::Error {
                target: target.clone(),
                message,
            }])
        }
    }
}

fn map_app_server_message(target: &OutboundTarget, message: &AppServerMessage) -> EventFlow {
    match message {
        AppServerMessage::Notification(notification) => map_notification(target, notification),
        AppServerMessage::Request(_) => EventFlow::Continue(Vec::new()),
    }
}

fn map_notification(target: &OutboundTarget, notification: &AppServerNotification) -> EventFlow {
    match notification {
        AppServerNotification::AgentMessageDelta { delta, .. } => {
            EventFlow::Continue(vec![GatewayOutbound::TextDelta {
                target: target.clone(),
                delta: delta.clone(),
            }])
        }
        AppServerNotification::TurnCompleted {
            conversation_id,
            turn_id,
        } => {
            info!(
                conversation_id = %conversation_id,
                turn_id = %turn_id,
                "gateway.runtime.event.turn_completed"
            );
            EventFlow::Completed(vec![GatewayOutbound::FlushText {
                target: target.clone(),
            }])
        }
        AppServerNotification::TurnFailed {
            conversation_id,
            turn_id,
            error,
        } => {
            info!(
                conversation_id = %conversation_id,
                turn_id = %turn_id,
                error_preview = %preview(error, 120),
                "gateway.runtime.event.turn_failed"
            );
            EventFlow::Completed(vec![GatewayOutbound::Error {
                target: target.clone(),
                message: error.clone(),
            }])
        }
        AppServerNotification::TurnCancelled {
            conversation_id,
            turn_id,
            reason,
        } => {
            info!(
                conversation_id = %conversation_id,
                turn_id = %turn_id,
                reason_preview = %preview(reason, 120),
                "gateway.runtime.event.turn_cancelled"
            );
            EventFlow::Completed(vec![GatewayOutbound::Info {
                target: target.clone(),
                message: format!("本轮已取消: {reason}"),
            }])
        }
        AppServerNotification::ItemCompleted {
            conversation_id,
            turn_id,
            call_id,
            item: TranscriptItem::AgentMessage { text, .. },
            ..
        } => {
            info!(
                conversation_id = %conversation_id,
                turn_id = %turn_id,
                call_id = ?call_id,
                text_chars = text.chars().count(),
                text_preview = %preview(text, 120),
                "gateway.runtime.event.agent_message_completed"
            );
            EventFlow::Continue(vec![GatewayOutbound::FinalText {
                target: target.clone(),
                text: text.clone(),
            }])
        }
        AppServerNotification::ItemStarted { kind, title, .. } => {
            started_item_to_outbound(target, kind, title.as_deref())
        }
        AppServerNotification::PlanDelta { delta, .. } => {
            progress_outbound(target, GatewayProgressKind::Plan, delta, true, false)
        }
        AppServerNotification::ReasoningSummaryTextDelta { delta, .. }
        | AppServerNotification::ReasoningTextDelta { delta, .. } => {
            progress_outbound(target, GatewayProgressKind::Reasoning, delta, true, false)
        }
        AppServerNotification::ItemCompleted { item, .. } => {
            completed_item_to_outbound(target, item)
        }
        AppServerNotification::CommandExecutionOutputDelta { .. }
        | AppServerNotification::ToolOutputDelta { .. }
        | AppServerNotification::FileChangeOutputDelta { .. } => EventFlow::Continue(Vec::new()),
        AppServerNotification::Info {
            conversation_id,
            message,
        } => {
            info!(
                conversation_id = %conversation_id,
                message_preview = %preview(message, 120),
                "gateway.runtime.event.info"
            );
            EventFlow::Continue(vec![GatewayOutbound::Info {
                target: target.clone(),
                message: message.clone(),
            }])
        }
        AppServerNotification::Error {
            conversation_id,
            message,
        } => {
            info!(
                conversation_id = %conversation_id,
                message_preview = %preview(message, 120),
                "gateway.runtime.event.error"
            );
            EventFlow::Continue(vec![GatewayOutbound::Error {
                target: target.clone(),
                message: message.clone(),
            }])
        }
        _ => EventFlow::Continue(Vec::new()),
    }
}

fn completed_item_to_outbound(target: &OutboundTarget, item: &TranscriptItem) -> EventFlow {
    match item {
        TranscriptItem::Reasoning { title, text, .. } => {
            EventFlow::Continue(vec![GatewayOutbound::Progress(GatewayProgressUpdate {
                target: target.clone(),
                kind: GatewayProgressKind::Reasoning,
                summary: format_reasoning_summary(title, text),
                streaming: false,
            })])
        }
        TranscriptItem::FileChange { .. } => EventFlow::Continue(Vec::new()),
        TranscriptItem::ToolResult { .. } | TranscriptItem::CommandExecution { .. } => {
            EventFlow::Continue(Vec::new())
        }
        _ => EventFlow::Continue(Vec::new()),
    }
}

fn started_item_to_outbound(
    target: &OutboundTarget,
    kind: &TurnItemKind,
    title: Option<&str>,
) -> EventFlow {
    let Some(summary) = humanize_tool_stage(kind, title) else {
        return EventFlow::Continue(Vec::new());
    };
    EventFlow::Continue(vec![GatewayOutbound::Progress(GatewayProgressUpdate {
        target: target.clone(),
        kind: GatewayProgressKind::Tool,
        summary,
        streaming: false,
    })])
}

fn progress_outbound(
    target: &OutboundTarget,
    kind: GatewayProgressKind,
    raw: &str,
    streaming: bool,
    completed: bool,
) -> EventFlow {
    let summary = normalize_progress_text(raw);
    if summary.is_empty() {
        return if completed {
            EventFlow::Completed(Vec::new())
        } else {
            EventFlow::Continue(Vec::new())
        };
    }
    let outbound = GatewayOutbound::Progress(GatewayProgressUpdate {
        target: target.clone(),
        kind,
        summary,
        streaming,
    });
    if completed {
        EventFlow::Completed(vec![outbound])
    } else {
        EventFlow::Continue(vec![outbound])
    }
}

fn normalize_progress_text(raw: &str) -> String {
    let flattened = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    flattened.trim().to_string()
}

fn humanize_tool_stage(kind: &TurnItemKind, title: Option<&str>) -> Option<String> {
    match kind {
        TurnItemKind::CommandExecution => Some(humanize_command_stage(title.unwrap_or_default())),
        TurnItemKind::FileChange => Some("正在整理文件改动...".to_string()),
        TurnItemKind::ToolCall | TurnItemKind::ToolResult => {
            Some("正在调用工具处理中...".to_string())
        }
        _ => None,
    }
}

fn humanize_command_stage(command: &str) -> String {
    let normalized = command.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return "正在检查项目信息...".to_string();
    }
    if normalized.starts_with("git log") || normalized.starts_with("git show") {
        return "正在查看 Git 历史...".to_string();
    }
    if normalized.starts_with("git diff") {
        return "正在查看代码改动...".to_string();
    }
    if normalized.starts_with("git status") || normalized.starts_with("git branch") {
        return "正在检查仓库状态...".to_string();
    }
    if normalized.starts_with("cargo ")
        || normalized.starts_with("npm ")
        || normalized.starts_with("pnpm ")
        || normalized.starts_with("yarn ")
        || normalized.starts_with("pytest")
        || normalized.starts_with("go test")
    {
        return "正在运行构建或测试...".to_string();
    }
    if normalized.starts_with("rg ")
        || normalized.starts_with("find ")
        || normalized.starts_with("ls ")
        || normalized.starts_with("dir ")
        || normalized.starts_with("get-childitem")
        || normalized.starts_with("get-content")
        || normalized.starts_with("cat ")
    {
        return "正在查看项目文件...".to_string();
    }
    "正在处理项目内容...".to_string()
}

fn format_reasoning_summary(title: &str, text: &str) -> String {
    let title = title.trim();
    let text = normalize_progress_text(text);
    if title.is_empty() {
        return text;
    }
    if text.is_empty() {
        return title.to_string();
    }
    format!("{title}: {text}")
}

fn preview(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out.replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::{EventFlow, map_app_server_event};
    use crate::gateway_outbound::OutboundTarget;
    use agent_app_server_client::AppServerEvent;
    use agent_protocol::{AppServerMessage, AppServerNotification};

    fn target() -> OutboundTarget {
        OutboundTarget {
            conversation_id: "agent:main:feishu:dm:oc_1".to_string(),
            chat_id: "oc_1".to_string(),
            chat_type: Some("p2p".to_string()),
            is_reply_chain: false,
            reply_context: None,
        }
    }

    #[test]
    fn turn_failed_completes_with_error() {
        let event = AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::TurnFailed {
                conversation_id: "agent:main:feishu:dm:oc_1".to_string(),
                turn_id: "turn-1".to_string(),
                error: "boom".to_string(),
            },
        ));

        match map_app_server_event(&target(), event) {
            EventFlow::Completed(outbounds) => assert_eq!(outbounds.len(), 1),
            EventFlow::Continue(_) => panic!("turn failed should complete"),
        }
    }

    #[test]
    fn turn_cancelled_completes_with_info() {
        let event = AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::TurnCancelled {
                conversation_id: "agent:main:feishu:dm:oc_1".to_string(),
                turn_id: "turn-1".to_string(),
                reason: "cancelled".to_string(),
            },
        ));

        match map_app_server_event(&target(), event) {
            EventFlow::Completed(outbounds) => assert_eq!(outbounds.len(), 1),
            EventFlow::Continue(_) => panic!("turn cancelled should complete"),
        }
    }
}
