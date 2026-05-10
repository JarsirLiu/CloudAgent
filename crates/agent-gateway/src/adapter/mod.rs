pub mod feishu;
pub mod wecom;

use crate::{GatewayMessage, GatewayOutbound};
use anyhow::Result;
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub trait PlatformOutboundRenderer {
    type Output;

    fn render(&mut self, outbound: GatewayOutbound) -> Vec<Self::Output>;
}

pub struct PassthroughRenderer<T> {
    _marker: std::marker::PhantomData<T>,
}

impl<T> Default for PassthroughRenderer<T> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> PlatformOutboundRenderer for PassthroughRenderer<T>
where
    T: From<GatewayOutbound>,
{
    type Output = T;

    fn render(&mut self, outbound: GatewayOutbound) -> Vec<Self::Output> {
        vec![outbound.into()]
    }
}

pub fn spawn_platform_bridge<Client, Event, Renderer, Output, NextEventFn, SendFn, MapInboundFn>(
    mut client: Client,
    inbound_tx: mpsc::Sender<GatewayMessage>,
    mut outbound_rx: mpsc::Receiver<GatewayOutbound>,
    mut renderer: Renderer,
    mut next_event: NextEventFn,
    mut send_output: SendFn,
    mut map_inbound: MapInboundFn,
) -> JoinHandle<Result<()>>
where
    Client: Send + 'static,
    Event: Send + 'static,
    Output: Send + 'static,
    Renderer: PlatformOutboundRenderer<Output = Output> + Send + 'static,
    NextEventFn: for<'a> FnMut(
            &'a mut Client,
        ) -> Pin<Box<dyn Future<Output = Result<Option<Event>>> + Send + 'a>>
        + Send
        + 'static,
    SendFn: for<'a> FnMut(
            &'a mut Client,
            Output,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>
        + Send
        + 'static,
    MapInboundFn: FnMut(Event) -> Option<GatewayMessage> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            tokio::select! {
                event = next_event(&mut client) => {
                    let Some(event) = event? else {
                        break;
                    };
                    if let Some(message) = map_inbound(event) {
                        inbound_tx.send(message).await?;
                    }
                }
                outbound = outbound_rx.recv() => {
                    let Some(outbound) = outbound else {
                        break;
                    };
                    for rendered in renderer.render(outbound) {
                        send_output(&mut client, rendered).await?;
                    }
                }
            }
        }
        Ok(())
    })
}

#[async_trait]
pub trait GatewayAdapter: Send {
    async fn next_message(&mut self) -> Result<Option<GatewayMessage>>;

    async fn send_outbound(&mut self, outbound: GatewayOutbound) -> Result<()>;
}
