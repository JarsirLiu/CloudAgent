use crate::adapter::feishu::{FeishuAdapter, FeishuAdapterOptions};
use crate::app_server_mapping::{EventFlow, map_app_server_event};
use crate::config::GatewayConfig;
use crate::gateway_event::{GatewayEvent, OutboundTarget};
use crate::message::InboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use crate::session::build_session_key;
use agent_app_server_client::{AppServerClient, InProcessClientConfig};
use agent_core::{AgentHost, ApprovalPolicy, PermissionProfile, text_input_items};
use agent_protocol::{TurnPolicy, UserTurnInput};
use anyhow::Result;
use async_trait::async_trait;
use cli::agent_host::build_agent_host;
use config::AgentConfig;
use std::env;
use std::sync::Arc;
use tracing::{error, info};

pub async fn run_gateway(config: GatewayConfig) -> Result<()> {
    let adapter: Arc<dyn PlatformAdapter> = Arc::new(FeishuAdapter::new(
        config.clone(),
        FeishuAdapterOptions::default(),
    )?);
    let runtime = Arc::new(GatewayRuntime::new(config, adapter.clone())?);
    adapter.run(runtime).await
}

struct GatewayRuntime {
    adapter: Arc<dyn PlatformAdapter>,
    agent_host: Arc<AgentHost>,
}

impl GatewayRuntime {
    fn new(_config: GatewayConfig, adapter: Arc<dyn PlatformAdapter>) -> Result<Self> {
        let workspace_root = env::current_dir()?;
        let agent_config = AgentConfig::load_runtime(workspace_root)?;
        let agent_host = build_agent_host(agent_config)?;
        Ok(Self {
            adapter,
            agent_host,
        })
    }
}

#[async_trait]
impl MessageHandler for GatewayRuntime {
    async fn handle_message(&self, message: InboundMessage) -> Result<()> {
        let session_key = build_session_key(&message);
        let target = OutboundTarget {
            conversation_id: session_key.clone(),
            chat_id: message.chat_id.clone(),
            chat_type: message.chat_type.clone(),
            is_reply_chain: message.thread_id.is_some(),
            reply_context: message.reply_context.clone(),
        };
        info!(
            platform = %message.platform,
            chat_id = %message.chat_id,
            message_id = %message.message_id,
            session_key = %session_key,
            text_preview = %preview(&message.text, 120),
            "gateway.runtime.inbound.accepted"
        );
        info!(
            session_key = %session_key,
            "gateway.runtime.turn.start"
        );
        let mut client = AppServerClient::in_process(InProcessClientConfig {
            runtime: self.agent_host.clone(),
            conversation_id: session_key.clone(),
            auto_approve: false,
            auto_approve_reason: None,
        });

        if let Err(error) = client.submit_turn(UserTurnInput {
            conversation_id: session_key.clone(),
            content: text_input_items(message.text.clone()),
            turn_policy: TurnPolicy {
                permission_profile: PermissionProfile::ReadOnly,
                approval_policy: ApprovalPolicy::OnRequest,
            },
        }) {
            error!(?error, session_key = %session_key, "gateway.runtime.turn.submit_failed");
            self.adapter
                .send_event(GatewayEvent::Error {
                    target,
                    message: "消息已收到，但提交到 Agent 运行时失败。".to_string(),
                })
                .await?;
            return Ok(());
        }

        while let Some(event) = client.next_event().await {
            info!(session_key = %session_key, event = %event_name(&event), "gateway.runtime.event.received");
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

        if let Err(error) = client.shutdown().await {
            error!(?error, session_key = %session_key, "gateway.runtime.client.shutdown_failed");
        }

        Ok(())
    }
}

fn event_name(event: &agent_app_server_client::AppServerEvent) -> &'static str {
    match event {
        agent_app_server_client::AppServerEvent::Message(message) => match message {
            agent_protocol::AppServerMessage::Notification(notification) => match notification {
                agent_protocol::AppServerNotification::AgentMessageDelta { .. } => {
                    "agent_message_delta"
                }
                agent_protocol::AppServerNotification::PlanDelta { .. } => "plan_delta",
                agent_protocol::AppServerNotification::ReasoningTextDelta { .. } => {
                    "reasoning_text_delta"
                }
                agent_protocol::AppServerNotification::ReasoningSummaryTextDelta { .. } => {
                    "reasoning_summary_delta"
                }
                agent_protocol::AppServerNotification::CommandExecutionOutputDelta { .. } => {
                    "command_output_delta"
                }
                agent_protocol::AppServerNotification::ToolOutputDelta { .. } => {
                    "tool_output_delta"
                }
                agent_protocol::AppServerNotification::JsonPatchDelta { .. } => "json_patch_delta",
                agent_protocol::AppServerNotification::ItemCompleted { .. } => "item_completed",
                agent_protocol::AppServerNotification::TurnCompleted { .. } => "turn_completed",
                agent_protocol::AppServerNotification::Info { .. } => "info",
                agent_protocol::AppServerNotification::Error { .. } => "error",
                _ => "notification_other",
            },
            agent_protocol::AppServerMessage::Request(_) => "server_request",
        },
        agent_app_server_client::AppServerEvent::Lagged { .. } => "lagged",
        agent_app_server_client::AppServerEvent::Disconnected { .. } => "disconnected",
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
