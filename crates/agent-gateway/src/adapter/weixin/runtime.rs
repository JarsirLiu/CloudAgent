use super::client::WeixinAdapter;
use super::config::WeixinAdapterConfig;
use crate::app_server_mapping::{EventFlow, map_app_server_event};
use crate::gateway_event::{GatewayEvent, OutboundTarget};
use crate::message::InboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use crate::session::build_session_key;
use agent_app_server_client::{AppServerClient, AppServerEvent};
use agent_core::text_input_items;
use agent_protocol::{AppClientCommand, AppServerMessage, AppServerNotification, TurnPolicy, UserTurnInput};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};
use tracing::{debug, info, warn};

pub struct PlatformRuntime {
    task: JoinHandle<Result<()>>,
}

impl PlatformRuntime {
    pub async fn wait(self) -> Result<()> {
        self.task.await?
    }
}

pub fn spawn_runtime(
    config: WeixinAdapterConfig,
    node_client: AppServerClient,
    turn_policy: TurnPolicy,
) -> Result<PlatformRuntime> {
    let adapter = Arc::new(WeixinAdapter::new(config)?);
    let handler = Arc::new(NodeBackedHandler {
        adapter: adapter.clone(),
        stream_client: Mutex::new(node_client),
        turn_policy,
    });
    let platform_adapter: Arc<dyn PlatformAdapter> = adapter;
    let task = tokio::spawn(async move { platform_adapter.run(handler).await });
    Ok(PlatformRuntime { task })
}

struct NodeBackedHandler {
    adapter: Arc<WeixinAdapter>,
    stream_client: Mutex<AppServerClient>,
    turn_policy: TurnPolicy,
}

#[async_trait]
impl MessageHandler for NodeBackedHandler {
    async fn try_handle_session_command(&self, _message: &InboundMessage) -> Result<bool> {
        Ok(false)
    }

    async fn handle_message(&self, message: InboundMessage) -> Result<()> {
        let session_key = build_session_key(&message);
        let target = OutboundTarget {
            conversation_id: session_key.clone(),
            chat_id: message.chat_id.clone(),
            chat_type: message.chat_type.clone(),
            is_reply_chain: false,
            reply_context: None,
        };
        info!(
            session_key = %session_key,
            chat_id = %target.chat_id,
            text_preview = %preview(&message.text, 120),
            "weixin.runtime.inbound.accepted"
        );

        let mut stream_client = self.stream_client.lock().await;
        stream_client.send_command(AppClientCommand::SubscribeConversation {
            conversation_id: session_key.clone(),
        })?;
        stream_client.submit_turn(UserTurnInput {
            conversation_id: session_key.clone(),
            content: text_input_items(message.text.clone()),
            turn_policy: self.turn_policy.clone(),
        })?;

        let mut active_turn_id: Option<String> = None;
        loop {
            let maybe_event = timeout(Duration::from_secs(60), stream_client.next_event()).await;
            let event = match maybe_event {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => {
                    self.adapter
                        .send_event(GatewayEvent::Info {
                            target: target.clone(),
                            message: "消息已提交给 Agent，但后续事件返回超时。".to_string(),
                        })
                        .await?;
                    break;
                }
            };
            if event_conversation_id(&event) != Some(session_key.as_str()) {
                debug!(
                    session_key = %session_key,
                    event = %event_name(&event),
                    event_conversation_id = ?event_conversation_id(&event),
                    "weixin.runtime.event.ignored_different_conversation"
                );
                continue;
            }
            let event_turn_id = event_turn_id(&event);
            if let Some(bound_turn_id) = active_turn_id.as_deref() {
                if let Some(event_turn_id) = event_turn_id && event_turn_id != bound_turn_id {
                    debug!(
                        session_key = %session_key,
                        bound_turn_id,
                        event_turn_id,
                        event = %event_name(&event),
                        "weixin.runtime.event.ignored_different_turn"
                    );
                    continue;
                }
            } else if let Some(event_turn_id) = event_turn_id {
                if matches!(
                    &event,
                    AppServerEvent::Message(
                        AppServerMessage::Notification(AppServerNotification::TurnStarted { .. })
                    )
                ) {
                    active_turn_id = Some(event_turn_id.to_string());
                    info!(
                        session_key = %session_key,
                        turn_id = %event_turn_id,
                        "weixin.runtime.turn.bound"
                    );
                } else {
                    debug!(
                        session_key = %session_key,
                        event = %event_name(&event),
                        event_turn_id,
                        "weixin.runtime.event.ignored_before_turn_started"
                    );
                    continue;
                }
            }
            let event_name = event_name(&event);
            match map_app_server_event(&target, event) {
                EventFlow::Continue(outbounds) => {
                    log_outbounds(&session_key, event_name, &outbounds);
                    for event in outbounds {
                        self.adapter.send_event(event).await?;
                    }
                }
                EventFlow::Completed(outbounds) => {
                    log_outbounds(&session_key, event_name, &outbounds);
                    for event in outbounds {
                        self.adapter.send_event(event).await?;
                    }
                    break;
                }
            }
        }

        debug!(session_key = %session_key, "weixin.runtime.turn.completed");
        Ok(())
    }
}

fn event_conversation_id(event: &AppServerEvent) -> Option<&str> {
    match event {
        AppServerEvent::Message(message) => message.conversation_id(),
        AppServerEvent::Lagged { .. } | AppServerEvent::Disconnected { .. } => None,
    }
}

fn event_turn_id(event: &AppServerEvent) -> Option<&str> {
    match event {
        AppServerEvent::Message(AppServerMessage::Notification(notification)) => {
            notification_turn_id(notification)
        }
        AppServerEvent::Message(AppServerMessage::Request(_)) => None,
        AppServerEvent::Lagged { .. } | AppServerEvent::Disconnected { .. } => None,
    }
}

fn notification_turn_id(notification: &AppServerNotification) -> Option<&str> {
    match notification {
        AppServerNotification::TurnStarted { turn_id, .. }
        | AppServerNotification::ItemStarted { turn_id, .. }
        | AppServerNotification::AgentMessageDelta { turn_id, .. }
        | AppServerNotification::PlanDelta { turn_id, .. }
        | AppServerNotification::ReasoningSummaryTextDelta { turn_id, .. }
        | AppServerNotification::ReasoningTextDelta { turn_id, .. }
        | AppServerNotification::CommandExecutionOutputDelta { turn_id, .. }
        | AppServerNotification::ToolOutputDelta { turn_id, .. }
        | AppServerNotification::FileChangeOutputDelta { turn_id, .. }
        | AppServerNotification::TokenUsageUpdated { turn_id, .. }
        | AppServerNotification::ModelRetrying { turn_id, .. }
        | AppServerNotification::ItemCompleted { turn_id, .. }
        | AppServerNotification::TurnCompleted { turn_id, .. }
        | AppServerNotification::TurnFailed { turn_id, .. }
        | AppServerNotification::TurnCancelled { turn_id, .. } => Some(turn_id.as_str()),
        _ => None,
    }
}

fn event_name(event: &AppServerEvent) -> &'static str {
    match event {
        AppServerEvent::Message(AppServerMessage::Notification(notification)) => match notification {
            AppServerNotification::TurnStarted { .. } => "turn_started",
            AppServerNotification::ItemStarted { .. } => "item_started",
            AppServerNotification::AgentMessageDelta { .. } => "agent_message_delta",
            AppServerNotification::PlanDelta { .. } => "plan_delta",
            AppServerNotification::ReasoningSummaryTextDelta { .. } => "reasoning_summary_delta",
            AppServerNotification::ReasoningTextDelta { .. } => "reasoning_text_delta",
            AppServerNotification::CommandExecutionOutputDelta { .. } => "command_output_delta",
            AppServerNotification::ToolOutputDelta { .. } => "tool_output_delta",
            AppServerNotification::FileChangeOutputDelta { .. } => "file_change_output_delta",
            AppServerNotification::ItemCompleted { item, .. } => match item {
                agent_core::TranscriptItem::AgentMessage { .. } => "agent_message_completed",
                agent_core::TranscriptItem::Reasoning { .. } => "reasoning_completed",
                agent_core::TranscriptItem::CommandExecution { .. } => "command_completed",
                agent_core::TranscriptItem::FileChange { .. } => "file_change_completed",
                agent_core::TranscriptItem::ToolResult { .. } => "tool_result_completed",
                _ => "item_completed_other",
            },
            AppServerNotification::TurnCompleted { .. } => "turn_completed",
            AppServerNotification::TurnFailed { .. } => "turn_failed",
            AppServerNotification::TurnCancelled { .. } => "turn_cancelled",
            AppServerNotification::Info { .. } => "info",
            AppServerNotification::Error { .. } => "error",
            _ => "notification_other",
        },
        AppServerEvent::Message(AppServerMessage::Request(_)) => "server_request",
        AppServerEvent::Lagged { .. } => "lagged",
        AppServerEvent::Disconnected { .. } => "disconnected",
    }
}

fn log_outbounds(session_key: &str, event_name: &str, outbounds: &[GatewayEvent]) {
    if outbounds.is_empty() {
        debug!(
            session_key = %session_key,
            event = event_name,
            "weixin.runtime.outbound.empty"
        );
        return;
    }

    for outbound in outbounds {
        match outbound {
            GatewayEvent::ItemDelta { kind, delta, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = ?kind,
                chars = delta.chars().count(),
                preview = %preview(delta, 120),
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::TurnCompleted { .. } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "turn_completed",
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ItemCompleted { item, .. } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "item_completed",
                item = ?item,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ServerRequestRequested { request, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = "server_request_requested",
                request = ?request,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ServerRequestResolved {
                request_id,
                decision,
                ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = "server_request_resolved",
                request_id = ?request_id,
                decision = ?decision,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::TokenUsageUpdated {
                total_usage,
                model_context_window,
                ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = "token_usage_updated",
                total_usage = ?total_usage,
                model_context_window = ?model_context_window,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ModelRetrying {
                stage,
                attempt,
                next_delay_ms,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "model_retrying",
                stage = ?stage,
                attempt,
                next_delay_ms,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ContextCompactionStarted {
                continuation,
                estimated_tokens,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "context_compaction_started",
                continuation = ?continuation,
                estimated_tokens,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ContextCompacted {
                continuation,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "context_compacted",
                continuation = ?continuation,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ItemStarted { item, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = ?item,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::TurnStarted { turn_id, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                turn_id,
                kind = "turn_started",
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::TurnFailed { error, .. } => warn!(
                session_key = %session_key,
                event = event_name,
                kind = "turn_failed",
                preview = %preview(error, 120),
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::TurnCancelled { reason, .. } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "turn_cancelled",
                preview = %preview(reason, 120),
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::Info { message, .. } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "info",
                preview = %preview(message, 120),
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::Error { message, .. } => warn!(
                session_key = %session_key,
                event = event_name,
                kind = "error",
                preview = %preview(message, 120),
                "weixin.runtime.outbound.generated"
            ),
        }
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
