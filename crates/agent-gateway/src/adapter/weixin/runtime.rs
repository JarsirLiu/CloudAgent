use super::client::WeixinAdapter;
use super::config::WeixinAdapterConfig;
use crate::adapter::runtime_shared::{
    build_outbound_target, build_turn_content, event_conversation_id, event_name, event_turn_id,
};
use crate::app_server_mapping::{EventFlow, map_app_server_event};
use crate::gateway_event::GatewayEvent;
use crate::message::InboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use crate::session::build_session_key;
use agent_app_server_client::{AppServerClient, AppServerEvent};
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, TurnPolicy, UserTurnInput,
};
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
        let target = build_outbound_target(
            session_key.clone(),
            message.chat_id.clone(),
            message.chat_type.clone(),
            None,
            false,
        );
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
            content: build_turn_content(&message),
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
                if let Some(event_turn_id) = event_turn_id
                    && event_turn_id != bound_turn_id
                {
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
                    AppServerEvent::Message(AppServerMessage::Notification(
                        AppServerNotification::TurnStarted { .. }
                    ))
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
            GatewayEvent::ItemProgress {
                item_id, progress, ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = "item_progress",
                item_id,
                progress = ?progress,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ItemMetricsUpdated {
                item_id, metrics, ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = "item_metrics_updated",
                item_id,
                metrics = ?metrics,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ReasoningSummaryPartAdded {
                item_id,
                summary_index,
                ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                kind = "reasoning_summary_part_added",
                item_id,
                summary_index,
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
                phase,
                estimated_tokens,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "context_compaction_started",
                phase = ?phase,
                estimated_tokens,
                "weixin.runtime.outbound.generated"
            ),
            GatewayEvent::ContextCompacted {
                phase,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                kind = "context_compacted",
                phase = ?phase,
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
