use crate::{
    AppServerEvent, DEFAULT_EVENT_CHANNEL_CAPACITY,
    stdio::{read_events_from_with_disconnect_message, write_commands_to},
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
        let reader_task = tokio::spawn(async move {
            read_events_from_with_disconnect_message(
                BufReader::new(read_half),
                event_tx,
                "local node closed",
            )
            .await
        });

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

#[cfg(test)]
mod tests {
    use super::{LocalNodeAppServerClient, LocalNodeClientConfig};
    use crate::AppServerEvent;
    use agent_protocol::{
        AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerMessageEnvelope,
        AppServerNotification, JsonRpcMessage,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn local_node_client_sends_commands_and_receives_events() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("local addr");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept client");
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half).lines();

            let line = reader
                .next_line()
                .await
                .expect("read command line")
                .expect("command payload");
            let rpc: JsonRpcMessage = serde_json::from_str(&line).expect("jsonrpc command");
            let envelope = AppClientCommandEnvelope::try_from(rpc).expect("command envelope");
            assert!(matches!(
                envelope.command,
                AppClientCommand::ListConversations
            ));

            let payload = serde_json::to_string(&JsonRpcMessage::from(AppServerMessageEnvelope {
                message: AppServerMessage::Notification(AppServerNotification::Info {
                    conversation_id: "default".to_string(),
                    message: "hello from node".to_string(),
                }),
                event_seq: Some(1),
            }))
            .expect("serialize event");
            write_half
                .write_all(payload.as_bytes())
                .await
                .expect("write event");
            write_half.write_all(b"\n").await.expect("write newline");
            write_half.flush().await.expect("flush event");
        });

        let mut client = LocalNodeAppServerClient::connect(LocalNodeClientConfig {
            address: address.to_string(),
        })
        .await
        .expect("connect client");

        client
            .send_command(AppClientCommand::ListConversations)
            .expect("send command");

        match client.next_event().await.expect("info event") {
            AppServerEvent::Message(AppServerMessage::Notification(
                AppServerNotification::Info { message, .. },
            )) => assert_eq!(message, "hello from node"),
            other => panic!("unexpected event: {other:?}"),
        }

        match client.next_event().await.expect("disconnect event") {
            AppServerEvent::Disconnected { message } => {
                assert_eq!(message, "local node closed");
            }
            other => panic!("unexpected disconnect event: {other:?}"),
        }

        client.shutdown().await.expect("shutdown client");
        server.await.expect("server task");
    }

    #[tokio::test]
    async fn local_node_client_shutdown_closes_writer_side() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("local addr");
        let saw_eof = Arc::new(AtomicBool::new(false));
        let saw_eof_server = saw_eof.clone();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept client");
            let (read_half, _) = stream.into_split();
            let mut reader = BufReader::new(read_half).lines();
            while reader
                .next_line()
                .await
                .expect("read client stream")
                .is_some()
            {}
            saw_eof_server.store(true, Ordering::SeqCst);
        });

        let client = LocalNodeAppServerClient::connect(LocalNodeClientConfig {
            address: address.to_string(),
        })
        .await
        .expect("connect client");

        client.shutdown().await.expect("shutdown client");
        server.await.expect("server task");
        assert!(saw_eof.load(Ordering::SeqCst));
    }
}
