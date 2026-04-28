mod in_process;
mod stdio;

use agent_protocol::AppServerMessage;
use anyhow::Result;

pub use in_process::InProcessClientConfig;
pub use stdio::StdioClientConfig;

pub enum AppServerClient {
    InProcess(in_process::InProcessAppServerClient),
    Stdio(stdio::StdioAppServerClient),
}

impl AppServerClient {
    pub fn in_process(config: InProcessClientConfig) -> Self {
        Self::InProcess(in_process::InProcessAppServerClient::start(config))
    }

    pub async fn stdio(config: StdioClientConfig) -> Result<Self> {
        Ok(Self::Stdio(
            stdio::StdioAppServerClient::spawn(config).await?,
        ))
    }

    pub fn send_command(&self, command: agent_protocol::AppClientCommand) -> Result<()> {
        match self {
            Self::InProcess(client) => client.send_command(command),
            Self::Stdio(client) => client.send_command(command),
        }
    }

    pub async fn next_message(&mut self) -> Option<AppServerMessage> {
        match self {
            Self::InProcess(client) => client.next_message().await,
            Self::Stdio(client) => client.next_message().await,
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        match self {
            Self::InProcess(client) => client.shutdown().await,
            Self::Stdio(client) => client.shutdown().await,
        }
    }
}
