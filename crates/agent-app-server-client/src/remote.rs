use crate::{AppServerConnectInfo, AppServerEvent, TypedRequestError};
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessageEnvelope, ConversationListResponse,
    JsonRpcError, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId, TransportClientInfo, TransportInitializeCapabilities,
    TransportInitializeParams, TransportInitializeResult,
};
use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;
use std::{io, io::ErrorKind};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

#[derive(Clone, Debug)]
pub struct RemoteClientConfig {
    pub address: String,
    pub client: AppServerConnectInfo,
    pub connect_timeout: Duration,
    pub initialize_timeout: Duration,
}

impl RemoteClientConfig {
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

pub struct RemoteAppServerClient {
    command_tx: mpsc::UnboundedSender<RemoteOutbound>,
    event_rx: mpsc::Receiver<AppServerEvent>,
    pending_events: VecDeque<AppServerEvent>,
    request_counter: Arc<AtomicI64>,
    writer_task: JoinHandle<Result<()>>,
    reader_task: JoinHandle<Result<()>>,
}

#[derive(Clone)]
pub struct RemoteAppServerRequestHandle {
    command_tx: mpsc::UnboundedSender<RemoteOutbound>,
    request_counter: Arc<AtomicI64>,
}

enum RemoteOutbound {
    Command(AppClientCommand),
    Request {
        request: JsonRpcRequest,
        response_tx: oneshot::Sender<Result<JsonRpcResponseEnvelope, io::Error>>,
    },
    Shutdown,
}

enum JsonRpcResponseEnvelope {
    Result(serde_json::Value),
    Error(JsonRpcErrorPayload),
}

impl RemoteAppServerClient {
    pub async fn connect(config: RemoteClientConfig) -> Result<Self> {
        let stream = timeout(config.connect_timeout, TcpStream::connect(&config.address))
            .await
            .map_err(|_| {
                anyhow!(
                    "timed out connecting to remote app server at {}",
                    config.address
                )
            })?
            .map_err(|err| {
                anyhow!(
                    "failed to connect to remote app server at {}: {err}",
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
        .map_err(|_| {
            anyhow!(
                "timed out initializing remote app server at {}",
                config.address
            )
        })??;
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let event_capacity = config.client.channel_capacity.max(1);
        let (event_tx, event_rx) = mpsc::channel(event_capacity);
        let request_counter = Arc::new(AtomicI64::new(1));
        let request_counter_for_writer = request_counter.clone();
        let pending_requests = Arc::new(Mutex::new(HashMap::<
            RequestId,
            oneshot::Sender<Result<JsonRpcResponseEnvelope, io::Error>>,
        >::new()));
        let pending_requests_for_writer = pending_requests.clone();
        let pending_requests_for_reader = pending_requests.clone();

        let writer_task = tokio::spawn(async move {
            write_outbound_messages_to(
                &mut writer,
                &mut command_rx,
                request_counter_for_writer,
                pending_requests_for_writer,
            )
            .await
        });
        let reader_task = tokio::spawn(async move {
            read_transport_messages_from(
                reader,
                event_tx,
                "remote app server closed",
                pending_requests_for_reader,
            )
            .await
        });

        Ok(Self {
            command_tx,
            event_rx,
            pending_events,
            request_counter,
            writer_task,
            reader_task,
        })
    }

    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(RemoteOutbound::Command(command))
            .map_err(|_| anyhow!("remote app server command channel is closed"))
    }

    pub fn request_handle(&self) -> RemoteAppServerRequestHandle {
        RemoteAppServerRequestHandle {
            command_tx: self.command_tx.clone(),
            request_counter: self.request_counter.clone(),
        }
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        self.request_handle().request_typed(request).await
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
        let RemoteAppServerClient {
            command_tx,
            event_rx: _,
            pending_events: _,
            request_counter: _,
            writer_task,
            reader_task,
        } = self;

        let _ = command_tx.send(RemoteOutbound::Shutdown);
        drop(command_tx);
        writer_task.await??;
        reader_task.await??;
        Ok(())
    }
}

impl RemoteAppServerRequestHandle {
    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(RemoteOutbound::Command(command))
            .map_err(|_| anyhow!("remote app server command channel is closed"))
    }

    pub async fn request_conversation_list(&self) -> Result<ConversationListResponse> {
        self.request_typed(JsonRpcRequest {
            id: RequestId::Integer(
                self.request_counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ),
            method: "conversation/list".to_string(),
            params: None,
        })
        .await
        .map_err(anyhow::Error::from)
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        let method = request.method.clone();
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(RemoteOutbound::Request {
                request,
                response_tx,
            })
            .map_err(|_| TypedRequestError::Transport {
                method: method.clone(),
                source: io::Error::new(
                    ErrorKind::BrokenPipe,
                    "remote app server command channel is closed",
                ),
            })?;

        let response = response_rx
            .await
            .map_err(|_| TypedRequestError::Transport {
                method: method.clone(),
                source: io::Error::new(
                    ErrorKind::BrokenPipe,
                    "remote app server request channel is closed",
                ),
            })?
            .map_err(|source| TypedRequestError::Transport {
                method: method.clone(),
                source,
            })?;

        let value = match response {
            JsonRpcResponseEnvelope::Result(value) => value,
            JsonRpcResponseEnvelope::Error(source) => {
                return Err(TypedRequestError::Server { method, source });
            }
        };

        serde_json::from_value(value)
            .map_err(|source| TypedRequestError::Deserialize { method, source })
    }
}

async fn write_outbound_messages_to<W>(
    writer: &mut W,
    command_rx: &mut mpsc::UnboundedReceiver<RemoteOutbound>,
    request_counter: Arc<AtomicI64>,
    pending_requests: Arc<
        Mutex<HashMap<RequestId, oneshot::Sender<Result<JsonRpcResponseEnvelope, io::Error>>>>,
    >,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(outbound) = command_rx.recv().await {
        match outbound {
            RemoteOutbound::Command(command) => {
                let envelope = AppClientCommandEnvelope {
                    request_id: RequestId::Integer(
                        request_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    ),
                    command,
                };
                let payload = serde_json::to_string(&JsonRpcMessage::from(envelope))?;
                writer.write_all(payload.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
            RemoteOutbound::Request {
                request,
                response_tx,
            } => {
                pending_requests
                    .lock()
                    .await
                    .insert(request.id.clone(), response_tx);
                let payload = serde_json::to_string(&JsonRpcMessage::Request(request))?;
                writer.write_all(payload.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
            RemoteOutbound::Shutdown => break,
        }
    }
    Ok(())
}

async fn read_transport_messages_from<R>(
    reader: R,
    event_tx: mpsc::Sender<AppServerEvent>,
    disconnect_message: &str,
    pending_requests: Arc<
        Mutex<HashMap<RequestId, oneshot::Sender<Result<JsonRpcResponseEnvelope, io::Error>>>>,
    >,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
{
    let mut lines = reader.lines();
    let mut skipped_events = 0usize;
    let mut last_seq_by_conversation: HashMap<String, u64> = HashMap::new();

    while let Some(line) = lines.next_line().await? {
        let message: JsonRpcMessage = serde_json::from_str(&line)
            .context("failed to parse remote app-server transport event")?;
        match message {
            JsonRpcMessage::Response(JsonRpcResponse { id, result }) => {
                if let Some(response_tx) = pending_requests.lock().await.remove(&id) {
                    let _ = response_tx.send(Ok(JsonRpcResponseEnvelope::Result(result)));
                }
            }
            JsonRpcMessage::Error(JsonRpcError { id, error }) => {
                if let Some(response_tx) = pending_requests.lock().await.remove(&id) {
                    let _ = response_tx.send(Ok(JsonRpcResponseEnvelope::Error(error)));
                }
            }
            JsonRpcMessage::Notification(_) | JsonRpcMessage::Request(_) => {
                let envelope = AppServerMessageEnvelope::try_from(message)?;
                if let (Some(conversation_id), Some(event_seq)) =
                    (envelope.message.conversation_id(), envelope.event_seq)
                {
                    let last_seq = last_seq_by_conversation
                        .entry(conversation_id.to_string())
                        .or_insert(0);
                    if event_seq <= *last_seq {
                        continue;
                    }
                    *last_seq = event_seq;
                }

                if !crate::forward_event(
                    &event_tx,
                    &mut skipped_events,
                    AppServerEvent::Message(envelope.message),
                )
                .await
                {
                    return Ok(());
                }
            }
        }
    }

    let mut pending = pending_requests.lock().await;
    for (_, response_tx) in pending.drain() {
        let _ = response_tx.send(Err(io::Error::new(
            ErrorKind::BrokenPipe,
            disconnect_message.to_string(),
        )));
    }
    drop(pending);

    let _ = crate::forward_event(
        &event_tx,
        &mut skipped_events,
        AppServerEvent::Disconnected {
            message: disconnect_message.to_string(),
        },
    )
    .await;
    Ok(())
}

async fn perform_initialize_handshake<R, W>(
    reader: &mut R,
    writer: &mut W,
    config: &RemoteClientConfig,
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
                anyhow::bail!("remote app server initialize failed: {}", error.message)
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
        anyhow::bail!("remote app server closed during initialize")
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
    use super::{RemoteAppServerClient, RemoteClientConfig};
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

    fn test_config(address: String) -> RemoteClientConfig {
        RemoteClientConfig {
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
                    transport: "remote".to_string(),
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

        let mut client = RemoteAppServerClient::connect(test_config(address.to_string()))
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
                assert_eq!(message, "remote app server closed");
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
                    transport: "remote".to_string(),
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

        let client = RemoteAppServerClient::connect(test_config(address.to_string()))
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

        let error = match RemoteAppServerClient::connect(test_config(address.to_string())).await {
            Ok(_) => panic!("connect should fail when initialize is rejected"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("remote app server initialize failed"),
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
        let error = match RemoteAppServerClient::connect(config).await {
            Ok(_) => panic!("connect should time out when initialize hangs"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("timed out initializing remote app server"),
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
                        transport: "remote".to_string(),
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

        let mut client = RemoteAppServerClient::connect(test_config(address.to_string()))
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

    #[tokio::test]
    async fn local_node_request_handle_reads_conversation_list_response() {
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
            let JsonRpcMessage::Request(JsonRpcRequest { id, method, .. }) =
                serde_json::from_str(&line).expect("initialize jsonrpc")
            else {
                panic!("expected initialize request");
            };
            assert_eq!(method, "initialize");

            let initialize_response =
                serde_json::to_string(&JsonRpcMessage::Response(JsonRpcResponse {
                    id,
                    result: serde_json::to_value(TransportInitializeResult {
                        server_info: agent_protocol::TransportServerInfo {
                            name: "gatewayd".to_string(),
                            version: "0.0.0-test".to_string(),
                        },
                        protocol_version: "1".to_string(),
                        transport: "remote".to_string(),
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
                .expect("read list request")
                .expect("list request payload");
            let JsonRpcMessage::Request(JsonRpcRequest { id, method, .. }) =
                serde_json::from_str(&line).expect("conversation list jsonrpc")
            else {
                panic!("expected list request");
            };
            assert_eq!(method, "conversation/list");

            let response = serde_json::to_string(&JsonRpcMessage::Response(JsonRpcResponse {
                id,
                result: serde_json::json!({
                    "conversations": [
                        {
                            "conversation_id": "conversation-1",
                            "title": "Alpha",
                            "message_count": 3,
                            "updated_at_ms": 42
                        }
                    ]
                }),
            }))
            .expect("serialize list response");
            write_half
                .write_all(response.as_bytes())
                .await
                .expect("write list response");
            write_half
                .write_all(b"\n")
                .await
                .expect("write list newline");
            write_half.flush().await.expect("flush list response");
        });

        let client = RemoteAppServerClient::connect(test_config(address.to_string()))
            .await
            .expect("connect client");
        let request_handle = client.request_handle();
        let response = request_handle
            .request_conversation_list()
            .await
            .expect("conversation list response");

        assert_eq!(response.conversations.len(), 1);
        assert_eq!(response.conversations[0].conversation_id, "conversation-1");
        assert_eq!(response.conversations[0].title.as_deref(), Some("Alpha"));

        client.shutdown().await.expect("shutdown client");
        server.await.expect("server task");
    }
}
