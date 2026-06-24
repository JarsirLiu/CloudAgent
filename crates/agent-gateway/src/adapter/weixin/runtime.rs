use super::client::WeixinAdapter;
use super::config::WeixinAdapterConfig;
use crate::adapter::runtime_event_log::{log_outbound_events, preview};
use crate::adapter::runtime_shared::{
    RuntimeSessionGate, RuntimeSessionState, build_outbound_target, build_turn_content,
    event_conversation_id, event_name,
};
use crate::app_server_mapping::{EventFlow, map_app_server_event};
use crate::gateway_event::GatewayEvent;
use crate::message::InboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use crate::session::build_session_key;
use agent_app_server_client::AppServerClient;
use agent_protocol::{AppClientCommand, TurnPolicy, UserTurnInput};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};
use tracing::{debug, info};

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

        let mut session_state = RuntimeSessionState::new();
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
            match session_state.gate_event(&session_key, &event) {
                RuntimeSessionGate::Accepted => {}
                RuntimeSessionGate::ForeignConversation => {
                    debug!(
                        session_key = %session_key,
                        event = %event_name(&event),
                        event_conversation_id = ?event_conversation_id(&event),
                        "weixin.runtime.event.ignored_different_conversation"
                    );
                    continue;
                }
                RuntimeSessionGate::ForeignTurn {
                    bound_turn_id,
                    event_turn_id,
                } => {
                    debug!(
                        session_key = %session_key,
                        bound_turn_id,
                        event_turn_id,
                        event = %event_name(&event),
                        "weixin.runtime.event.ignored_different_turn"
                    );
                    continue;
                }
                RuntimeSessionGate::BeforeTurnStarted { event_turn_id } => {
                    debug!(
                        session_key = %session_key,
                        event = %event_name(&event),
                        event_turn_id,
                        "weixin.runtime.event.ignored_before_turn_started"
                    );
                    continue;
                }
                RuntimeSessionGate::BoundTurn { turn_id } => {
                    info!(
                        session_key = %session_key,
                        turn_id,
                        "weixin.runtime.turn.bound"
                    );
                }
            }
            let event_name = event_name(&event);
            match map_app_server_event(&target, event) {
                EventFlow::Continue(outbounds) => {
                    log_outbound_events(&session_key, event_name, &outbounds, "weixin");
                    for event in outbounds {
                        self.adapter.send_event(event).await?;
                    }
                }
                EventFlow::Completed(outbounds) => {
                    log_outbound_events(&session_key, event_name, &outbounds, "weixin");
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

#[cfg(test)]
pub(crate) fn build_turn_content_for_tests(message: &InboundMessage) -> Vec<agent_core::InputItem> {
    build_turn_content(message)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
