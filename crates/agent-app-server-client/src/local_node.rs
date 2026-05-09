use crate::{
    AppServerEvent, DEFAULT_EVENT_CHANNEL_CAPACITY,
    stdio::{read_events_from_with_disconnect_message, write_commands_to},
};
use agent_protocol::{
    AppClientCommand, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId, TransportClientInfo, TransportInitializeCapabilities,
    TransportInitializeParams, TransportInitializeResult,
};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
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
        let mut reader = BufReader::new(read_half);
        let mut writer = write_half;
        perform_initialize_handshake(&mut reader, &mut writer).await?;
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel(DEFAULT_EVENT_CHANNEL_CAPACITY);
        let request_counter = Arc::new(AtomicI64::new(1));

        let writer_task = tokio::spawn(async move {
            write_commands_to(&mut writer, &mut command_rx, request_counter).await
        });
        let reader_task = tokio::spawn(async move {
            read_events_from_with_disconnect_message(reader, event_tx, "local node closed").await
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

async fn perform_initialize_handshake<R, W>(reader: &mut R, writer: &mut W) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    write_jsonrpc_message(
        writer,
        JsonRpcMessage::Request(JsonRpcRequest {
            id: RequestId::String("initialize".to_string()),
            method: "initialize".to_string(),
            params: Some(serde_json::to_value(TransportInitializeParams {
                client_info: TransportClientInfo {
                    name: "cloudagent-cli".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                capabilities: Some(TransportInitializeCapabilities {
                    experimental_api: true,
                    opt_out_notification_methods: Vec::new(),
                }),
            })?),
        }),
    )
    .await?;

    let response = read_jsonrpc_message(reader).await?;
    match response {
        JsonRpcMessage::Response(JsonRpcResponse { id, result })
            if id == RequestId::String("initialize".to_string()) =>
        {
            let _: TransportInitializeResult = serde_json::from_value(result)?;
        }
        JsonRpcMessage::Error(JsonRpcError { error, .. }) => {
            anyhow::bail!("local node initialize failed: {}", error.message)
        }
        other => anyhow::bail!("unexpected local node initialize response: {other:?}"),
    }

    write_jsonrpc_message(
        writer,
        JsonRpcMessage::Notification(JsonRpcNotification {
            method: "initialized".to_string(),
            params: None,
        }),
    )
    .await?;
    Ok(())
}

async fn read_jsonrpc_message<R>(reader: &mut R) -> Result<JsonRpcMessage>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await?;
    if bytes == 0 {
        anyhow::bail!("local node closed during initialize")
    }
    Ok(serde_json::from_str(line.trim_end())?)
}

async fn write_jsonrpc_message<W>(writer: &mut W, message: JsonRpcMessage) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_string(&message)?;
    writer.write_all(payload.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{LocalNodeAppServerClient, LocalNodeClientConfig};
    use crate::AppServerEvent;
    use agent_protocol::{
        AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerMessageEnvelope,
        AppServerNotification, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
        JsonRpcResponse, TransportInitializeParams, TransportInitializeResult,
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
                .expect("read initialize line")
                .expect("initialize payload");
            let JsonRpcMessage::Request(JsonRpcRequest { id, method, params }) =
                serde_json::from_str(&line).expect("initialize jsonrpc")
            else {
                panic!("expected initialize request");
            };
            assert_eq!(method, "initialize");
            let _: TransportInitializeParams =
                serde_json::from_value(params.expect("initialize params"))
                    .expect("initialize params should decode");
            let payload = serde_json::to_string(&JsonRpcMessage::Response(JsonRpcResponse {
                id,
                result: serde_json::to_value(TransportInitializeResult {
                    server_info: agent_protocol::TransportServerInfo {
                        name: "gatewayd".to_string(),
                        version: "0.0.0-test".to_string(),
                    },
                    protocol_version: "1".to_string(),
                    transport: "local-node".to_string(),
                })
                .expect("serialize initialize result"),
            }))
            .expect("serialize initialize response");
            write_half
                .write_all(payload.as_bytes())
                .await
                .expect("write initialize response");
            write_half
                .write_all(b"\n")
                .await
                .expect("write initialize newline");
            write_half.flush().await.expect("flush initialize response");

            let line = reader
                .next_line()
                .await
                .expect("read initialized line")
                .expect("initialized payload");
            let JsonRpcMessage::Notification(JsonRpcNotification { method, .. }) =
                serde_json::from_str(&line).expect("initialized jsonrpc")
            else {
                panic!("expected initialized notification");
            };
            assert_eq!(method, "initialized");

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
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half).lines();

            let line = reader
                .next_line()
                .await
                .expect("read initialize line")
                .expect("initialize payload");
            let JsonRpcMessage::Request(JsonRpcRequest { id, method, params }) =
                serde_json::from_str(&line).expect("initialize jsonrpc")
            else {
                panic!("expected initialize request");
            };
            assert_eq!(method, "initialize");
            let _: TransportInitializeParams =
                serde_json::from_value(params.expect("initialize params"))
                    .expect("initialize params should decode");

            let line = serde_json::to_string(&JsonRpcMessage::Response(JsonRpcResponse {
                id,
                result: serde_json::to_value(TransportInitializeResult {
                    server_info: agent_protocol::TransportServerInfo {
                        name: "gatewayd".to_string(),
                        version: "0.0.0-test".to_string(),
                    },
                    protocol_version: "1".to_string(),
                    transport: "local-node".to_string(),
                })
                .expect("serialize initialize result"),
            }))
            .expect("serialize initialize response");
            write_half
                .write_all(line.as_bytes())
                .await
                .expect("write initialize response");
            write_half
                .write_all(b"\n")
                .await
                .expect("write initialize newline");
            write_half.flush().await.expect("flush initialize response");

            let line = reader
                .next_line()
                .await
                .expect("read initialized line")
                .expect("initialized payload");
            let JsonRpcMessage::Notification(JsonRpcNotification { method, .. }) =
                serde_json::from_str(&line).expect("initialized jsonrpc")
            else {
                panic!("expected initialized notification");
            };
            assert_eq!(method, "initialized");

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
