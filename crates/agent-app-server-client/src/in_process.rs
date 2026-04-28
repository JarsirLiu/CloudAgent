use agent_app_server::{InProcessClientHandle, InProcessClientSender, start_in_process};
use agent_protocol::{AppClientCommand, AppServerMessage};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct InProcessClientConfig {
    pub runtime: Arc<AgentRuntime>,
    pub session_id: String,
    pub auto_approve: bool,
    pub auto_approve_reason: Option<String>,
}

pub struct InProcessAppServerClient {
    sender: InProcessClientSender,
    handle: InProcessClientHandle,
}

impl InProcessAppServerClient {
    pub fn start(config: InProcessClientConfig) -> Self {
        let handle = start_in_process(
            config.runtime,
            config.session_id,
            config.auto_approve,
            config.auto_approve_reason,
        );
        let sender = handle.sender();
        Self { sender, handle }
    }

    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.sender.send_command(command)
    }

    pub async fn next_message(&mut self) -> Option<AppServerMessage> {
        self.handle.next_message().await
    }

    pub async fn shutdown(self) -> Result<()> {
        self.handle.shutdown().await
    }
}
