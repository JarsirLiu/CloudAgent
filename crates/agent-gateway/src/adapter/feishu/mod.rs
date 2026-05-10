mod client;
mod config;
mod inbound;
mod outbound;
mod render;

use crate::adapter::GatewayAdapter;
use crate::adapter::spawn_platform_bridge;
use crate::default_poll_interval;
use crate::direct::{DirectGatewaySession, PumpStatus};
use crate::{GatewayMessage, GatewayOutbound};
use agent_app_server_client::AppServerClient;
use agent_protocol::TurnPolicy;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

pub use client::{FeishuPlatformClient, FeishuPlatformEvent};
pub use config::FeishuAdapterConfig;
pub use inbound::{FeishuInboundMessage, FeishuReplyContext};
pub use outbound::{FeishuOutboundMessage, FeishuProgressKind};
use render::FeishuOutboundRenderer;

pub struct FeishuGatewayAdapter {
    inbound_rx: mpsc::Receiver<GatewayMessage>,
    outbound_tx: mpsc::Sender<GatewayOutbound>,
}

impl FeishuGatewayAdapter {
    pub fn new(
        inbound_rx: mpsc::Receiver<GatewayMessage>,
        outbound_tx: mpsc::Sender<GatewayOutbound>,
    ) -> Self {
        Self {
            inbound_rx,
            outbound_tx,
        }
    }
}

#[async_trait::async_trait]
impl GatewayAdapter for FeishuGatewayAdapter {
    async fn next_message(&mut self) -> Result<Option<GatewayMessage>> {
        Ok(self.inbound_rx.recv().await)
    }

    async fn send_outbound(&mut self, outbound: GatewayOutbound) -> Result<()> {
        self.outbound_tx.send(outbound).await?;
        Ok(())
    }
}

pub struct FeishuRuntimeHandle {
    bridge_task: JoinHandle<Result<()>>,
    session_task: JoinHandle<Result<PumpStatus>>,
}

impl FeishuRuntimeHandle {
    pub async fn wait(self) -> Result<PumpStatus> {
        let mut bridge_task = self.bridge_task;
        let mut session_task = self.session_task;
        tokio::select! {
            bridge = &mut bridge_task => {
                session_task.abort();
                match bridge {
                    Ok(Ok(())) => anyhow::bail!("feishu bridge task exited unexpectedly"),
                    Ok(Err(err)) => Err(err),
                    Err(err) => Err(err.into()),
                }
            }
            session = &mut session_task => {
                let status = session??;
                bridge_task.abort();
                let _ = bridge_task.await;
                Ok(status)
            }
        }
    }
}

pub fn spawn_runtime(
    config: FeishuAdapterConfig,
    node_client: AppServerClient,
    turn_policy: TurnPolicy,
) -> Result<FeishuRuntimeHandle> {
    let reply_contexts = Arc::new(Mutex::new(HashMap::new()));
    let client = FeishuPlatformClient::new(config, Arc::clone(&reply_contexts))?;
    let (inbound_tx, inbound_rx) = mpsc::channel(128);
    let (outbound_tx, outbound_rx) = mpsc::channel(128);
    let mut session = DirectGatewaySession::new(
        FeishuGatewayAdapter::new(inbound_rx, outbound_tx),
        node_client,
        turn_policy,
    );
    let bridge_task = spawn_platform_bridge(
        client,
        inbound_tx,
        outbound_rx,
        FeishuOutboundRenderer::default(),
        |client| Box::pin(client.next_platform_event()),
        |client, rendered| Box::pin(client.send_platform_message(rendered)),
        move |event| {
            if let FeishuPlatformEvent::Message(message) = event {
                let conversation_id = message.conversation_id().to_string();
                let reply_context = message.reply_context().clone();
                if let Ok(mut contexts) = reply_contexts.lock() {
                    contexts.insert(conversation_id, reply_context);
                } else {
                    warn!("feishu reply context store is poisoned");
                }
                info!("feishu runtime forwarding inbound message to gateway session");
                return Some(message.into_gateway_message());
            }
            None
        },
    );

    let session_task =
        tokio::spawn(async move { session.run_until_closed(default_poll_interval()).await });

    Ok(FeishuRuntimeHandle {
        bridge_task,
        session_task,
    })
}
