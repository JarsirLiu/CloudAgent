use super::client::WecomAdapter;
use super::config::WecomAdapterConfig;
use crate::adapter::runtime_shared::{
    build_outbound_target, build_turn_content, event_conversation_id, event_turn_id,
    parse_approval_command, render_request_prompt, render_request_resolution_label,
};
use crate::app_server_mapping::{EventFlow, map_app_server_event};
use crate::gateway_event::{GatewayEvent, OutboundTarget};
use crate::message::InboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use crate::session::build_session_key;
use agent_app_server_client::{AppServerClient, AppServerEvent, AppServerRequestHandle};
use agent_core::ServerRequestDecision;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, TurnPolicy,
    UserTurnInput,
};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
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
    config: WecomAdapterConfig,
    node_client: AppServerClient,
    turn_policy: TurnPolicy,
) -> Result<PlatformRuntime> {
    let adapter = Arc::new(WecomAdapter::new(config)?);
    let approvals = Arc::new(ApprovalCoordinator::new(node_client.request_handle()));
    let handler = Arc::new(NodeBackedHandler {
        adapter: adapter.clone(),
        stream_client: Mutex::new(node_client),
        approvals,
        turn_policy,
    });
    let platform_adapter: Arc<dyn PlatformAdapter> = adapter;
    let task = tokio::spawn(async move { platform_adapter.run(handler).await });
    Ok(PlatformRuntime { task })
}

struct NodeBackedHandler {
    adapter: Arc<WecomAdapter>,
    stream_client: Mutex<AppServerClient>,
    approvals: Arc<ApprovalCoordinator>,
    turn_policy: TurnPolicy,
}

#[derive(Clone)]
struct PendingApprovalRequest {
    request_id: agent_protocol::RequestId,
    request: agent_core::ServerRequest,
}

struct ApprovalCoordinator {
    request_handle: AppServerRequestHandle,
    state: Mutex<HashMap<String, PendingApprovalRequest>>,
}

impl ApprovalCoordinator {
    fn new(request_handle: AppServerRequestHandle) -> Self {
        Self {
            request_handle,
            state: Mutex::new(HashMap::new()),
        }
    }

    async fn register_pending(&self, session_key: &str, request: &AppServerRequest) {
        let AppServerRequest::ServerRequest {
            request_id,
            request,
            ..
        } = request;
        self.state.lock().await.insert(
            session_key.to_string(),
            PendingApprovalRequest {
                request_id: request_id.clone(),
                request: request.clone(),
            },
        );
    }

    async fn has_pending(&self, session_key: &str) -> bool {
        self.state.lock().await.contains_key(session_key)
    }

    async fn resolve_by_session(
        &self,
        session_key: &str,
        decision: ServerRequestDecision,
    ) -> Result<Option<PendingApprovalRequest>> {
        let pending = self.state.lock().await.remove(session_key);
        let Some(pending) = pending else {
            return Ok(None);
        };
        self.request_handle.resolve_server_request(
            session_key.to_string(),
            pending.request_id.clone(),
            decision,
        )?;
        Ok(Some(pending))
    }
}

#[async_trait]
impl MessageHandler for NodeBackedHandler {
    async fn try_handle_session_command(&self, message: &InboundMessage) -> Result<bool> {
        let Some(decision) = parse_approval_command(&message.text) else {
            return Ok(false);
        };
        let session_key = build_session_key(message);
        let pending = self
            .approvals
            .resolve_by_session(&session_key, decision.clone())
            .await?;
        let Some(pending) = pending else {
            return Ok(false);
        };

        self.adapter
            .send_event(GatewayEvent::Info {
                target: OutboundTarget {
                    conversation_id: session_key,
                    chat_id: message.chat_id.clone(),
                    chat_type: message.chat_type.clone(),
                    is_reply_chain: false,
                    reply_context: message.reply_context.clone(),
                },
                message: format!(
                    "审批已处理: {}",
                    render_request_resolution_label(&pending.request)
                ),
            })
            .await?;
        Ok(true)
    }

    async fn handle_message(&self, message: InboundMessage) -> Result<()> {
        let session_key = build_session_key(&message);
        let target = build_outbound_target(
            session_key.clone(),
            message.chat_id.clone(),
            message.chat_type.clone(),
            message.reply_context.clone(),
            false,
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
            let wait_duration = if self.approvals.has_pending(&session_key).await {
                Duration::from_secs(600)
            } else {
                Duration::from_secs(60)
            };
            let maybe_event = timeout(wait_duration, stream_client.next_event()).await;
            let event = match maybe_event {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(_) => {
                    self.adapter
                        .send_event(GatewayEvent::Info {
                            target: target.clone(),
                            message: if self.approvals.has_pending(&session_key).await {
                                "Agent 正在等待你在企微里回复 /approve、/always、/deny 或 /cancel。"
                                    .to_string()
                            } else {
                                "消息已提交给 Agent，但后续事件返回超时。".to_string()
                            },
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
                if let Some(event_turn_id) = event_turn_id
                    && event_turn_id != bound_turn_id
                {
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
                } else {
                    continue;
                }
            }
            if let Some(request) = event_request(&event) {
                self.approvals.register_pending(&session_key, request).await;
                self.adapter
                    .send_event(GatewayEvent::Info {
                        target: target.clone(),
                        message: format!(
                            "{}\n回复 /approve、/always、/deny 或 /cancel 继续。",
                            render_request_prompt(request)
                        ),
                    })
                    .await?;
                continue;
            }
            match map_app_server_event(&target, event) {
                EventFlow::Continue(outbounds) => {
                    for event in outbounds {
                        self.adapter.send_event(event).await?;
                    }
                }
                EventFlow::Completed(outbounds) => {
                    for event in outbounds {
                        self.adapter.send_event(event).await?;
                    }
                    break;
                }
            }
        }

        debug!(session_key = %session_key, "wecom.runtime.turn.completed");
        Ok(())
    }
}

fn event_request(event: &AppServerEvent) -> Option<&AppServerRequest> {
    match event {
        AppServerEvent::Message(AppServerMessage::Request(request)) => Some(request),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn build_turn_content_for_tests(
    message: &InboundMessage,
) -> Vec<agent_core::InputItem> {
    build_turn_content(message)
}
