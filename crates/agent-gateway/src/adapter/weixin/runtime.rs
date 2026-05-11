use super::client::WeixinAdapter;
use super::config::WeixinAdapterConfig;
use crate::app_server_mapping::{EventFlow, map_app_server_event};
use crate::gateway_outbound::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate, OutboundTarget};
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
use tracing::debug;

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

        self.adapter
            .send_outbound(GatewayOutbound::Progress(GatewayProgressUpdate {
                target: target.clone(),
                kind: GatewayProgressKind::Reasoning,
                summary: "模型开始处理当前消息".to_string(),
                streaming: true,
            }))
            .await?;

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
                        .send_outbound(GatewayOutbound::Info {
                            target: target.clone(),
                            message: "消息已提交给 Agent，但后续事件返回超时。".to_string(),
                        })
                        .await?;
                    break;
                }
            };
            if event_conversation_id(&event) != Some(session_key.as_str()) {
                continue;
            }
            let event_turn_id = event_turn_id(&event);
            if let Some(bound_turn_id) = active_turn_id.as_deref() {
                if let Some(event_turn_id) = event_turn_id && event_turn_id != bound_turn_id {
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
                } else {
                    continue;
                }
            }
            match map_app_server_event(&target, event) {
                EventFlow::Continue(outbounds) => {
                    for outbound in outbounds {
                        self.adapter.send_outbound(outbound).await?;
                    }
                }
                EventFlow::Completed(outbounds) => {
                    for outbound in outbounds {
                        self.adapter.send_outbound(outbound).await?;
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
