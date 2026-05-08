use crate::{
    AppServerEvent, DEFAULT_EVENT_CHANNEL_CAPACITY, stdio::read_events_from,
    stdio::write_commands_to,
};
use agent_protocol::AppClientCommand;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Clone, Debug)]
pub struct LocalNodeClientConfig {
    pub address: String,
}

pub struct LocalNodeAppServerClient {
    command_tx: mpsc::UnboundedSender<AppClientCommand>,
    event_rx: mpsc::Receiver<AppServerEvent>,
    writer_task: JoinHandle<Result<()>>,
    reader_task: JoinHandle<Result<()>>,
}

impl LocalNodeAppServerClient {
    pub async fn connect(config: LocalNodeClientConfig) -> Result<Self> {
        let stream = TcpStream::connect(&config.address).await?;
        let (read_half, write_half) = stream.into_split();
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel(DEFAULT_EVENT_CHANNEL_CAPACITY);
        let request_counter = Arc::new(AtomicI64::new(1));

        let writer_task = tokio::spawn(async move {
            let mut writer = write_half;
            write_commands_to(&mut writer, &mut command_rx, request_counter).await
        });
        let reader_task =
            tokio::spawn(
                async move { read_events_from(BufReader::new(read_half), event_tx).await },
            );

        Ok(Self {
            command_tx,
            event_rx,
            writer_task,
            reader_task,
        })
    }

    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .map_err(|_| anyhow!("local node command channel is closed"))
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        self.event_rx.recv().await
    }

    pub fn try_next_event(&mut self) -> Option<AppServerEvent> {
        self.event_rx.try_recv().ok()
    }

    pub async fn shutdown(self) -> Result<()> {
        let LocalNodeAppServerClient {
            command_tx,
            event_rx: _,
            writer_task,
            reader_task,
        } = self;

        drop(command_tx);
        writer_task.await??;
        reader_task.await??;
        Ok(())
    }
}
