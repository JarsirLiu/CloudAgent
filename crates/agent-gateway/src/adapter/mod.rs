use crate::{GatewayMessage, GatewayOutbound};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait GatewayAdapter: Send {
    async fn next_message(&mut self) -> Result<Option<GatewayMessage>>;

    async fn send_outbound(&mut self, outbound: GatewayOutbound) -> Result<()>;
}
