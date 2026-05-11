use crate::gateway_event::{GatewayEvent, GatewayItemDeltaKind, OutboundTarget};
use agent_app_server_client::AppServerEvent;
use agent_protocol::{AppServerMessage, AppServerNotification};
use tracing::info;

pub enum EventFlow {
    Continue(Vec<GatewayEvent>),
    Completed(Vec<GatewayEvent>),
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
            EventFlow::Continue(vec![GatewayEvent::Info {
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
            EventFlow::Completed(vec![GatewayEvent::Error {
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
        AppServerNotification::TurnStarted { turn_id, .. } => {
            EventFlow::Continue(vec![GatewayEvent::TurnStarted {
                target: target.clone(),
                turn_id: turn_id.clone(),
            }])
        }
        AppServerNotification::ItemStarted {
            turn_id,
            item_id,
            call_id,
            kind,
            title,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemStarted {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: call_id.clone(),
            kind: kind.clone(),
            title: title.clone(),
        }]),
        AppServerNotification::AgentMessageDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemDelta {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: None,
            kind: GatewayItemDeltaKind::AgentMessage,
            delta: delta.clone(),
        }]),
        AppServerNotification::PlanDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemDelta {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: None,
            kind: GatewayItemDeltaKind::Plan,
            delta: delta.clone(),
        }]),
        AppServerNotification::ReasoningSummaryTextDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemDelta {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: None,
            kind: GatewayItemDeltaKind::ReasoningSummary,
            delta: delta.clone(),
        }]),
        AppServerNotification::ReasoningTextDelta {
            turn_id,
            item_id,
            delta,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemDelta {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: None,
            kind: GatewayItemDeltaKind::ReasoningText,
            delta: delta.clone(),
        }]),
        AppServerNotification::CommandExecutionOutputDelta {
            turn_id,
            item_id,
            call_id,
            delta,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemDelta {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: call_id.clone(),
            kind: GatewayItemDeltaKind::CommandExecutionOutput,
            delta: delta.clone(),
        }]),
        AppServerNotification::ToolOutputDelta {
            turn_id,
            item_id,
            call_id,
            delta,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemDelta {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: call_id.clone(),
            kind: GatewayItemDeltaKind::ToolOutput,
            delta: delta.clone(),
        }]),
        AppServerNotification::FileChangeOutputDelta {
            turn_id,
            item_id,
            call_id,
            delta,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemDelta {
            target: target.clone(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            call_id: call_id.clone(),
            kind: GatewayItemDeltaKind::FileChangeOutput,
            delta: delta.clone(),
        }]),
        AppServerNotification::ItemCompleted {
            turn_id,
            call_id,
            item,
            ..
        } => EventFlow::Continue(vec![GatewayEvent::ItemCompleted {
            target: target.clone(),
            turn_id: turn_id.clone(),
            call_id: call_id.clone(),
            item: item.clone(),
        }]),
        AppServerNotification::TurnCompleted {
            conversation_id,
            turn_id,
        } => {
            info!(
                conversation_id = %conversation_id,
                turn_id = %turn_id,
                "gateway.runtime.event.turn_completed"
            );
            EventFlow::Completed(vec![GatewayEvent::TurnCompleted {
                target: target.clone(),
                turn_id: turn_id.clone(),
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
            EventFlow::Completed(vec![GatewayEvent::TurnFailed {
                target: target.clone(),
                turn_id: turn_id.clone(),
                error: error.clone(),
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
            EventFlow::Completed(vec![GatewayEvent::TurnCancelled {
                target: target.clone(),
                turn_id: turn_id.clone(),
                reason: reason.clone(),
            }])
        }
        AppServerNotification::Info {
            conversation_id,
            message,
        } => {
            info!(
                conversation_id = %conversation_id,
                message_preview = %preview(message, 120),
                "gateway.runtime.event.info"
            );
            EventFlow::Continue(vec![GatewayEvent::Info {
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
            EventFlow::Continue(vec![GatewayEvent::Error {
                target: target.clone(),
                message: message.clone(),
            }])
        }
        _ => EventFlow::Continue(Vec::new()),
    }
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
