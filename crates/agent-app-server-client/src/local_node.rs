use crate::{
    AppServerConnectInfo, AppServerEvent,
    stdio::{read_events_from_with_disconnect_message, write_commands_to},
};
use agent_protocol::{
    AppClientCommand, AppServerMessageEnvelope, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, RequestId, TransportClientInfo,
    TransportInitializeCapabilities, TransportInitializeParams, TransportInitializeResult,
};
use anyhow::{Result, anyhow};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

#[derive(Clone, Debug)]
pub struct LocalNodeClientConfig {
    pub address: String,
    pub client: AppServerConnectInfo,
    pub connect_timeout: Duration,
    pub initialize_timeout: Duration,
}

impl LocalNodeClientConfig {
    fn initialize_params(&self) -> TransportInitializeParams {
        TransportInitializeParams {
            client_info: TransportClientInfo {
                name: self.client.client_name.clone(),
                version: self.client.client_version.clone(),
            },
            capabilities: Some(TransportInitializeCapabilities {
                experimental_api: self.client.experimental_api,
                opt_out_notification_methods: self.client.opt_out_notification_methods.clone(),
            }),
        }
    }
}

pub struct LocalNodeAppServerClient {
    command_tx: mpsc::UnboundedSender<AppClientCommand>,
    event_rx: mpsc::Receiver<AppServerEvent>,
    pending_events: VecDeque<AppServerEvent>,
    writer_task: JoinHandle<Result<()>>,
    reader_task: JoinHandle<Result<()>>,
}

impl LocalNodeAppServerClient {
    pub async fn connect(config: LocalNodeClientConfig) -> Result<Self> {
        let stream = timeout(config.connect_timeout, TcpStream::connect(&config.address))
            .await
            .map_err(|_| anyhow!("timed out connecting to local node at {}", config.address))?
            .map_err(|err| {
                anyhow!(
                    "failed to connect to local node at {}: {err}",
                    config.address
                )
            })?;
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = write_half;
        let pending_events = timeout(
            config.initialize_timeout,
            perform_initialize_handshake(&mut reader, &mut writer, &config),
        )
        .await
        .map_err(|_| anyhow!("timed out initializing local node at {}", config.address))??;
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let event_capacity = config.client.channel_capacity.max(1);
        let (event_tx, event_rx) = mpsc::channel(event_capacity);
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
            pending_events,
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
        if let Some(event) = self.pending_events.pop_front() {
            return Some(event);
        }
        self.event_rx.recv().await
    }

    pub fn try_next_event(&mut self) -> Option<AppServerEvent> {
        if let Some(event) = self.pending_events.pop_front() {
            return Some(event);
        }
        self.event_rx.try_recv().ok()
    }

    pub async fn shutdown(self) -> Result<()> {
        let LocalNodeAppServerClient {
            command_tx,
            event_rx: _,
            pending_events: _,
            writer_task,
            reader_task,
        } = self;

        drop(command_tx);
        writer_task.await??;
        reader_task.await??;
        Ok(())
    }
}

async fn perform_initialize_handshake<R, W>(
    reader: &mut R,
    writer: &mut W,
    config: &LocalNodeClientConfig,
) -> Result<VecDeque<AppServerEvent>>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let initialize_request_id = RequestId::String("initialize".to_string());
    write_jsonrpc_message(
        writer,
        JsonRpcMessage::Request(JsonRpcRequest {
            id: initialize_request_id.clone(),
            method: "initialize".to_string(),
            params: Some(serde_json::to_value(config.initialize_params())?),
        }),
    )
    .await?;

    let mut pending_events = VecDeque::new();
    loop {
        let response = read_jsonrpc_message(reader).await?;
        match response {
            JsonRpcMessage::Response(JsonRpcResponse { id, result })
                if id == initialize_request_id =>
            {
                let _: TransportInitializeResult = serde_json::from_value(result)?;
                break;
            }
            JsonRpcMessage::Error(JsonRpcError { id, error }) if id == initialize_request_id => {
                anyhow::bail!("local node initialize failed: {}", error.message)
            }
            JsonRpcMessage::Notification(_) | JsonRpcMessage::Request(_) => {
                if let Some(event) = app_server_event_from_jsonrpc_message(response)? {
                    pending_events.push_back(event);
                }
            }
            JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {}
        }
    }

    write_jsonrpc_message(
        writer,
        JsonRpcMessage::Notification(JsonRpcNotification {
            method: "initialized".to_string(),
            params: None,
        }),
    )
    .await?;
    Ok(pending_events)
}

fn app_server_event_from_jsonrpc_message(
    message: JsonRpcMessage,
) -> Result<Option<AppServerEvent>> {
    let envelope = match AppServerMessageEnvelope::try_from(message) {
        Ok(envelope) => envelope,
        Err(_) => return Ok(None),
    };
    Ok(Some(AppServerEvent::Message(envelope.message)))
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
    use crate::{AppServerConnectInfo, AppServerEvent, DEFAULT_EVENT_CHANNEL_CAPACITY};
    use agent_protocol::{
        AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerMessageEnvelope,
        AppServerNotification, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
        JsonRpcResponse, TransportInitializeParams, TransportInitializeResult,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    fn test_config(address: String) -> LocalNodeClientConfig {
        LocalNodeClientConfig {
            address,
            client: AppServerConnectInfo {
                client_name: "cloudagent-cli".to_string(),
                client_version: "0.0.0-test".to_string(),
                experimental_api: true,
                opt_out_notification_methods: Vec::new(),
                channel_capacity: DEFAULT_EVENT_CHANNEL_CAPACITY,
            },
            connect_timeout: Duration::from_secs(5),
            initialize_timeout: Duration::from_secs(5),
        }
    }

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

        let mut client = LocalNodeAppServerClient::connect(test_config(address.to_string()))
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

        let client = LocalNodeAppServerClient::connect(test_config(address.to_string()))
            .await
            .expect("connect client");

        client.shutdown().await.expect("shutdown client");
        server.await.expect("server task");
        assert!(saw_eof.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn local_node_client_surfaces_initialize_rejection() {
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

            let payload =
                serde_json::to_string(&JsonRpcMessage::Error(agent_protocol::JsonRpcError {
                    id,
                    error: agent_protocol::JsonRpcErrorPayload {
                        code: -32000,
                        message: "initialize denied".to_string(),
                        data: None,
                    },
                }))
                .expect("serialize initialize error");
            write_half
                .write_all(payload.as_bytes())
                .await
                .expect("write initialize error");
            write_half
                .write_all(b"\n")
                .await
                .expect("write initialize newline");
            write_half.flush().await.expect("flush initialize error");
        });

        let error = match LocalNodeAppServerClient::connect(test_config(address.to_string())).await
        {
            Ok(_) => panic!("connect should fail when initialize is rejected"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("local node initialize failed"),
            "unexpected error: {error:#}"
        );

        server.await.expect("server task");
    }

    #[tokio::test]
    async fn local_node_client_times_out_when_initialize_hangs() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("local addr");

        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept client");
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let mut config = test_config(address.to_string());
        config.initialize_timeout = Duration::from_millis(50);
        let error = match LocalNodeAppServerClient::connect(config).await {
            Ok(_) => panic!("connect should time out when initialize hangs"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("timed out initializing local node"),
            "unexpected error: {error:#}"
        );

        server.await.expect("server task");
    }

    #[tokio::test]
    async fn local_node_client_buffers_events_emitted_during_initialize() {
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

            let pending_event =
                serde_json::to_string(&JsonRpcMessage::from(AppServerMessageEnvelope {
                    message: AppServerMessage::Notification(AppServerNotification::Info {
                        conversation_id: "default".to_string(),
                        message: "hello before ready".to_string(),
                    }),
                    event_seq: Some(1),
                }))
                .expect("serialize pending event");
            write_half
                .write_all(pending_event.as_bytes())
                .await
                .expect("write pending event");
            write_half
                .write_all(b"\n")
                .await
                .expect("write pending newline");

            let initialize_response =
                serde_json::to_string(&JsonRpcMessage::Response(JsonRpcResponse {
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
                .write_all(initialize_response.as_bytes())
                .await
                .expect("write initialize response");
            write_half
                .write_all(b"\n")
                .await
                .expect("write initialize newline");
            write_half.flush().await.expect("flush handshake payloads");

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
        });

        let mut client = LocalNodeAppServerClient::connect(test_config(address.to_string()))
            .await
            .expect("connect client");

        match client.next_event().await.expect("pending event") {
            AppServerEvent::Message(AppServerMessage::Notification(
                AppServerNotification::Info { message, .. },
            )) => assert_eq!(message, "hello before ready"),
            other => panic!("unexpected event: {other:?}"),
        }

        client.shutdown().await.expect("shutdown client");
        server.await.expect("server task");
    }
}
