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
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use client::{WecomPlatformClient, WecomPlatformEvent};
pub use config::WecomAdapterConfig;
pub use inbound::WecomInboundMessage;
pub use outbound::WecomOutboundMessage;
use render::WecomOutboundRenderer;

pub struct WecomGatewayAdapter {
    inbound_rx: mpsc::Receiver<GatewayMessage>,
    outbound_tx: mpsc::Sender<GatewayOutbound>,
}

impl WecomGatewayAdapter {
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
impl GatewayAdapter for WecomGatewayAdapter {
    async fn next_message(&mut self) -> Result<Option<GatewayMessage>> {
        Ok(self.inbound_rx.recv().await)
    }

    async fn send_outbound(&mut self, outbound: GatewayOutbound) -> Result<()> {
        self.outbound_tx.send(outbound).await?;
        Ok(())
    }
}

pub struct WecomRuntimeHandle {
    bridge_task: JoinHandle<Result<()>>,
    session_task: JoinHandle<Result<PumpStatus>>,
}

impl WecomRuntimeHandle {
    pub async fn wait(self) -> Result<PumpStatus> {
        let status = self.session_task.await??;
        self.bridge_task.abort();
        let _ = self.bridge_task.await;
        Ok(status)
    }
}

pub fn spawn_runtime(
    config: WecomAdapterConfig,
    node_client: AppServerClient,
    turn_policy: TurnPolicy,
) -> Result<WecomRuntimeHandle> {
    let client = WecomPlatformClient::new(config)?;
    let (inbound_tx, inbound_rx) = mpsc::channel(128);
    let (outbound_tx, outbound_rx) = mpsc::channel(128);
    let mut session = DirectGatewaySession::new(
        WecomGatewayAdapter::new(inbound_rx, outbound_tx),
        node_client,
        turn_policy,
    );
    let bridge_task = spawn_platform_bridge(
        client,
        inbound_tx,
        outbound_rx,
        WecomOutboundRenderer::default(),
        |client| Box::pin(client.next_platform_event()),
        |client, rendered| Box::pin(client.send_platform_message(rendered)),
        |event| {
            let WecomPlatformEvent::Message(message) = event;
            Some(message.into_gateway_message())
        },
    );

    let session_task =
        tokio::spawn(async move { session.run_until_closed(default_poll_interval()).await });

    Ok(WecomRuntimeHandle {
        bridge_task,
        session_task,
    })
}
