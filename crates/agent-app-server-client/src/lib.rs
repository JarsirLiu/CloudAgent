use agent_app_server::{
    InProcessClientHandle, InProcessClientSender, start_in_process,
};
use agent_protocol::{AppClientCommand, AppServerMessage};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use std::sync::Arc;

pub struct InProcessAppServerClient {
    sender: InProcessClientSender,
    handle: InProcessClientHandle,
}

impl InProcessAppServerClient {
    pub fn start(
        runtime: Arc<AgentRuntime>,
        session_id: String,
        auto_approve: bool,
        auto_approve_reason: Option<String>,
    ) -> Self {
        let handle = start_in_process(runtime, session_id, auto_approve, auto_approve_reason);
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
