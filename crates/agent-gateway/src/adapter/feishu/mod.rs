mod client;
mod config;
mod inbound;
mod outbound;

use crate::adapter::GatewayAdapter;
use crate::default_poll_interval;
use crate::direct::{DirectGatewaySession, PumpStatus};
use crate::{GatewayMessage, GatewayOutbound};
use agent_app_server_client::AppServerClient;
use agent_protocol::TurnPolicy;
use anyhow::Result;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use client::{FeishuPlatformClient, FeishuPlatformEvent};
pub use config::FeishuAdapterConfig;
pub use inbound::FeishuInboundMessage;
pub use outbound::FeishuOutboundMessage;

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
        let status = self.session_task.await??;
        self.bridge_task.abort();
        let _ = self.bridge_task.await;
        Ok(status)
    }
}

pub fn spawn_runtime(
    config: FeishuAdapterConfig,
    node_client: AppServerClient,
    turn_policy: TurnPolicy,
) -> Result<FeishuRuntimeHandle> {
    let mut client = FeishuPlatformClient::new(config)?;
    let (inbound_tx, inbound_rx) = mpsc::channel(128);
    let (outbound_tx, mut outbound_rx) = mpsc::channel(128);
    let mut session = DirectGatewaySession::new(
        FeishuGatewayAdapter::new(inbound_rx, outbound_tx),
        node_client,
        turn_policy,
    );

    let bridge_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                event = client.next_platform_event() => {
                    let Some(event) = event? else {
                        break;
                    };
                    if let FeishuPlatformEvent::Message(message) = event {
                        inbound_tx.send(message.into_gateway_message()).await?;
                    }
                }
                outbound = outbound_rx.recv() => {
                    let Some(outbound) = outbound else {
                        break;
                    };
                    client.send_platform_message(outbound.into()).await?;
                }
            }
        }
        Ok(())
    });

    let session_task =
        tokio::spawn(async move { session.run_until_closed(default_poll_interval()).await });

    Ok(FeishuRuntimeHandle {
        bridge_task,
        session_task,
    })
}
