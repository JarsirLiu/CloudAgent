use crate::gateway_outbound::GatewayOutbound;
use crate::message::InboundMessage;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait MessageHandler: Send + Sync {
    async fn handle_message(&self, message: InboundMessage) -> Result<()>;
    async fn try_handle_session_command(&self, _message: &InboundMessage) -> Result<bool> {
        Ok(false)
    }
}

#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    fn platform_name(&self) -> &'static str;
    async fn run(self: Arc<Self>, handler: Arc<dyn MessageHandler>) -> Result<()>;
    async fn send_outbound(&self, outbound: GatewayOutbound) -> Result<()>;
}
