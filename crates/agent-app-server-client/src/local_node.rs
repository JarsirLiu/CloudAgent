use crate::{AppServerEvent, stdio};
use anyhow::Result;
use std::ffi::OsString;

#[derive(Clone, Debug)]
pub struct LocalNodeClientConfig {
    pub program: OsString,
    pub args: Vec<OsString>,
}

pub struct LocalNodeAppServerClient {
    inner: stdio::StdioAppServerClient,
}

impl LocalNodeAppServerClient {
    pub async fn spawn(config: LocalNodeClientConfig) -> Result<Self> {
        let inner = stdio::StdioAppServerClient::spawn(stdio::StdioClientConfig {
            program: config.program,
            args: config.args,
        })
        .await?;
        Ok(Self { inner })
    }

    pub fn send_command(&self, command: agent_protocol::AppClientCommand) -> Result<()> {
        self.inner.send_command(command)
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        self.inner.next_event().await
    }

    pub fn try_next_event(&mut self) -> Option<AppServerEvent> {
        self.inner.try_next_event()
    }

    pub async fn shutdown(self) -> Result<()> {
        self.inner.shutdown().await
    }
}
