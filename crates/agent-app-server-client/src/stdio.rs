use crate::{AppServerEvent, DEFAULT_EVENT_CHANNEL_CAPACITY, TypedRequestError, forward_event};
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessageEnvelope, CommandExecutionContext,
    JsonRpcError, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RequestId,
};
use anyhow::{Context, Result, anyhow};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, ErrorKind};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

#[derive(Clone, Debug)]
pub struct StdioClientConfig {
    pub program: OsString,
    pub args: Vec<OsString>,
}

pub struct StdioAppServerClient {
    command_tx: mpsc::UnboundedSender<StdioOutbound>,
    event_rx: mpsc::Receiver<AppServerEvent>,
    child: Arc<Mutex<Child>>,
    reader_task: JoinHandle<Result<()>>,
}

#[derive(Clone)]
pub struct StdioAppServerRequestHandle {
    command_tx: mpsc::UnboundedSender<StdioOutbound>,
}

enum StdioOutbound {
    Command {
        command: AppClientCommand,
        context: Option<CommandExecutionContext>,
    },
    Request {
        request: JsonRpcRequest,
        response_tx: oneshot::Sender<Result<JsonRpcResponseEnvelope, io::Error>>,
    },
}

enum JsonRpcResponseEnvelope {
    Result(serde_json::Value),
    Error(JsonRpcErrorPayload),
}

type PendingRequestSender = oneshot::Sender<Result<JsonRpcResponseEnvelope, io::Error>>;
type PendingRequestMap = HashMap<RequestId, PendingRequestSender>;
type SharedPendingRequests = Arc<Mutex<PendingRequestMap>>;

impl StdioAppServerClient {
    pub async fn spawn(config: StdioClientConfig) -> Result<Self> {
        let mut command = Command::new(&config.program);
        command.args(&config.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());
        command.kill_on_drop(true);
        configure_background_stdio_child(&mut command);

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn {:?}", config.program))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("stdio app-server child missing stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("stdio app-server child missing stdout"))?;

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::channel(DEFAULT_EVENT_CHANNEL_CAPACITY);
        let request_counter = Arc::new(AtomicI64::new(1));
        let pending_requests = Arc::new(Mutex::new(PendingRequestMap::new()));

        tokio::spawn(write_commands(
            stdin,
            command_rx,
            request_counter.clone(),
            pending_requests.clone(),
        ));
        let reader_task = tokio::spawn(read_events(stdout, event_tx, pending_requests));

        Ok(Self {
            command_tx,
            event_rx,
            child: Arc::new(Mutex::new(child)),
            reader_task,
        })
    }

    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(StdioOutbound::Command {
                command,
                context: None,
            })
            .map_err(|_| anyhow!("stdio app-server command channel is closed"))
    }

    pub fn send_command_with_context(
        &self,
        command: AppClientCommand,
        context: Option<CommandExecutionContext>,
    ) -> Result<()> {
        self.command_tx
            .send(StdioOutbound::Command { command, context })
            .map_err(|_| anyhow!("stdio app-server command channel is closed"))
    }

    pub fn request_handle(&self) -> StdioAppServerRequestHandle {
        StdioAppServerRequestHandle {
            command_tx: self.command_tx.clone(),
        }
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        self.request_handle().request_typed(request).await
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        self.event_rx.recv().await
    }

    pub fn try_next_event(&mut self) -> Option<AppServerEvent> {
        self.event_rx.try_recv().ok()
    }

    pub async fn shutdown(self) -> Result<()> {
        let StdioAppServerClient {
            command_tx,
            event_rx: _,
            child,
            reader_task,
        } = self;

        let _ = command_tx.send(StdioOutbound::Command {
            command: AppClientCommand::Exit,
            context: None,
        });
        drop(command_tx);

        let mut child = child.lock().await;
        if child.try_wait()?.is_none()
            && timeout(Duration::from_secs(5), child.wait()).await.is_err()
        {
            child.kill().await.ok();
        }
        drop(child);
        reader_task.await??;
        Ok(())
    }
}

impl StdioAppServerRequestHandle {
    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(StdioOutbound::Command {
                command,
                context: None,
            })
            .map_err(|_| anyhow!("stdio app-server command channel is closed"))
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        let method = request.method.clone();
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(StdioOutbound::Request {
                request,
                response_tx,
            })
            .map_err(|_| TypedRequestError::Transport {
                method: method.clone(),
                source: io::Error::new(
                    ErrorKind::BrokenPipe,
                    "stdio app-server command channel is closed",
                ),
            })?;

        let response = response_rx
            .await
            .map_err(|_| TypedRequestError::Transport {
                method: method.clone(),
                source: io::Error::new(
                    ErrorKind::BrokenPipe,
                    "stdio app-server request channel is closed",
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

fn configure_background_stdio_child(_command: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        _command.creation_flags(CREATE_NO_WINDOW);
    }
}

async fn write_commands(
    mut stdin: ChildStdin,
    mut command_rx: mpsc::UnboundedReceiver<StdioOutbound>,
    request_counter: Arc<AtomicI64>,
    pending_requests: SharedPendingRequests,
) -> Result<()> {
    write_commands_to(
        &mut stdin,
        &mut command_rx,
        request_counter,
        pending_requests,
    )
    .await
}

async fn write_commands_to<W>(
    writer: &mut W,
    command_rx: &mut mpsc::UnboundedReceiver<StdioOutbound>,
    request_counter: Arc<AtomicI64>,
    pending_requests: SharedPendingRequests,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(outbound) = command_rx.recv().await {
        match outbound {
            StdioOutbound::Command { command, context } => {
                let envelope = AppClientCommandEnvelope {
                    request_id: RequestId::Integer(request_counter.fetch_add(1, Ordering::Relaxed)),
                    command,
                    context,
                };
                let payload = serde_json::to_string(&JsonRpcMessage::from(envelope))?;
                writer.write_all(payload.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
            StdioOutbound::Request {
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
        }
    }
    Ok(())
}

async fn read_events(
    stdout: tokio::process::ChildStdout,
    event_tx: mpsc::Sender<AppServerEvent>,
    pending_requests: SharedPendingRequests,
) -> Result<()> {
    read_events_from(BufReader::new(stdout), event_tx, pending_requests).await
}

async fn read_events_from<R>(
    reader: R,
    event_tx: mpsc::Sender<AppServerEvent>,
    pending_requests: SharedPendingRequests,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
{
    read_events_from_with_disconnect_message(
        reader,
        event_tx,
        "stdio app server closed",
        pending_requests,
    )
    .await
}

async fn read_events_from_with_disconnect_message<R>(
    reader: R,
    event_tx: mpsc::Sender<AppServerEvent>,
    disconnect_message: &str,
    pending_requests: SharedPendingRequests,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
{
    let mut lines = reader.lines();
    let mut skipped_events = 0usize;
    let mut last_seq_by_conversation: HashMap<String, u64> = HashMap::new();

    while let Some(line) = lines.next_line().await? {
        let message: JsonRpcMessage =
            serde_json::from_str(&line).context("failed to parse stdio app-server event")?;
        match message {
            JsonRpcMessage::Response(JsonRpcResponse { id, result }) => {
                if let Some(response_tx) = pending_requests.lock().await.remove(&id) {
                    let _ = response_tx.send(Ok(JsonRpcResponseEnvelope::Result(result)));
                }
                continue;
            }
            JsonRpcMessage::Error(JsonRpcError { id, error }) => {
                if let Some(response_tx) = pending_requests.lock().await.remove(&id) {
                    let _ = response_tx.send(Ok(JsonRpcResponseEnvelope::Error(error)));
                }
                continue;
            }
            JsonRpcMessage::Request(_) | JsonRpcMessage::Notification(_) => {}
        }
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

        if !forward_event(
            &event_tx,
            &mut skipped_events,
            AppServerEvent::Message(envelope.message),
        )
        .await
        {
            return Ok(());
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

    let _ = forward_event(
        &event_tx,
        &mut skipped_events,
        AppServerEvent::Disconnected {
            message: disconnect_message.to_string(),
        },
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ApprovalPolicy, CommandApprovalRequest, PermissionProfile};
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, JsonRpcMessage, TurnPolicy,
        UserTurnInput,
    };
    use tokio::io::duplex;

    #[tokio::test]
    async fn write_commands_serializes_jsonrpc_lines() {
        let (mut write_side, read_side) = duplex(4096);
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let counter = Arc::new(AtomicI64::new(7));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));

        command_tx
            .send(StdioOutbound::Command {
                command: AppClientCommand::SubmitTurn(UserTurnInput {
                    conversation_id: "default".to_string(),
                    content: vec![agent_core::InputItem::Text {
                        text: "hello".to_string(),
                    }],
                    turn_policy: TurnPolicy {
                        permission_profile: PermissionProfile::ReadOnly,
                        approval_policy: ApprovalPolicy::OnRequest,
                    },
                }),
                context: None,
            })
            .expect("queue command");
        drop(command_tx);

        write_commands_to(&mut write_side, &mut command_rx, counter, pending_requests)
            .await
            .expect("write commands");
        drop(write_side);

        let mut reader = BufReader::new(read_side);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read line");

        let rpc: JsonRpcMessage = serde_json::from_str(line.trim()).expect("jsonrpc");
        let envelope = AppClientCommandEnvelope::try_from(rpc).expect("command envelope");
        assert_eq!(envelope.request_id, RequestId::Integer(7));
        match envelope.command {
            AppClientCommand::SubmitTurn(input) => {
                assert_eq!(input.conversation_id, "default");
                assert_eq!(
                    input.content,
                    vec![agent_core::InputItem::Text {
                        text: "hello".to_string(),
                    }]
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_events_parses_notifications_and_requests() {
        let notification = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: "hello".to_string(),
            }),
            event_seq: None,
        };
        let request = AppServerMessageEnvelope {
            message: AppServerMessage::Request(agent_protocol::AppServerRequest::ServerRequest {
                request_id: RequestId::Integer(11),
                conversation_id: "default".to_string(),
                request: agent_core::ServerRequest::CommandApproval {
                    request: CommandApprovalRequest {
                        turn_id: "turn-1".to_string(),
                        tool_call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        reason: "need approval".to_string(),
                        command_preview: "{\"command\":\"pwd\"}".to_string(),
                    },
                },
            }),
            event_seq: None,
        };
        let payload = format!(
            "{}\n{}\n",
            serde_json::to_string(&JsonRpcMessage::from(notification)).expect("notification"),
            serde_json::to_string(&JsonRpcMessage::from(request)).expect("request"),
        );

        let (mut write_side, read_side) = duplex(4096);
        let writer = tokio::spawn(async move {
            write_side
                .write_all(payload.as_bytes())
                .await
                .expect("write payload");
        });
        let (event_tx, mut event_rx) = mpsc::channel(8);
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));

        read_events_from(BufReader::new(read_side), event_tx, pending_requests)
            .await
            .expect("read events");
        writer.await.expect("writer task");

        match event_rx.recv().await.expect("notification event") {
            AppServerEvent::Message(AppServerMessage::Notification(
                AppServerNotification::Info { message, .. },
            )) => assert_eq!(message, "hello"),
            other => panic!("unexpected first event: {other:?}"),
        }
        match event_rx.recv().await.expect("request event") {
            AppServerEvent::Message(AppServerMessage::Request(
                agent_protocol::AppServerRequest::ServerRequest {
                    request_id,
                    request: agent_core::ServerRequest::CommandApproval { request },
                    ..
                },
            )) => {
                assert_eq!(request_id, RequestId::Integer(11));
                assert_eq!(request.tool_name, "exec_command");
            }
            other => panic!("unexpected second event: {other:?}"),
        }
        match event_rx.recv().await.expect("disconnect event") {
            AppServerEvent::Disconnected { message } => {
                assert_eq!(message, "stdio app server closed");
            }
            other => panic!("unexpected third event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_events_dedupes_replayed_event_seq_per_conversation() {
        let first = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: "first".to_string(),
            }),
            event_seq: Some(1),
        };
        let replayed = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: "duplicate".to_string(),
            }),
            event_seq: Some(1),
        };
        let next = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: "second".to_string(),
            }),
            event_seq: Some(2),
        };
        let payload = format!(
            "{}\n{}\n{}\n",
            serde_json::to_string(&JsonRpcMessage::from(first)).expect("first"),
            serde_json::to_string(&JsonRpcMessage::from(replayed)).expect("replayed"),
            serde_json::to_string(&JsonRpcMessage::from(next)).expect("next"),
        );
        let (mut write_side, read_side) = duplex(4096);
        let writer = tokio::spawn(async move {
            write_side
                .write_all(payload.as_bytes())
                .await
                .expect("write payload");
        });
        let (event_tx, mut event_rx) = mpsc::channel(8);
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));

        read_events_from(BufReader::new(read_side), event_tx, pending_requests)
            .await
            .expect("read events");
        writer.await.expect("writer task");

        let mut messages = Vec::new();
        while let Some(event) = event_rx.recv().await {
            if let AppServerEvent::Message(AppServerMessage::Notification(
                AppServerNotification::Info { message, .. },
            )) = event
            {
                messages.push(message);
            }
        }
        assert_eq!(messages, vec!["first".to_string(), "second".to_string()]);
    }

    #[tokio::test]
    async fn request_handle_reads_typed_jsonrpc_response() {
        let (mut write_side, read_side) = duplex(4096);
        let writer = tokio::spawn(async move {
            let payload = serde_json::to_string(&JsonRpcMessage::Response(JsonRpcResponse {
                id: RequestId::Integer(21),
                result: serde_json::json!({ "conversations": [] }),
            }))
            .expect("response");
            write_side
                .write_all(format!("{payload}\n").as_bytes())
                .await
                .expect("write response");
        });
        let (event_tx, _event_rx) = mpsc::channel(8);
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let reader_task = tokio::spawn(read_events_from(
            BufReader::new(read_side),
            event_tx,
            pending_requests.clone(),
        ));
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (request_tx, request_rx) = oneshot::channel();
        command_tx
            .send(StdioOutbound::Request {
                request: JsonRpcRequest {
                    id: RequestId::Integer(21),
                    method: "conversation/list".to_string(),
                    params: None,
                },
                response_tx: request_tx,
            })
            .expect("queue request");
        drop(command_tx);
        let (mut sink, _unused_read_side) = duplex(4096);
        write_commands_to(
            &mut sink,
            &mut command_rx,
            Arc::new(AtomicI64::new(1)),
            pending_requests,
        )
        .await
        .expect("write request");
        let response = request_rx
            .await
            .expect("response channel")
            .expect("ok response");
        match response {
            JsonRpcResponseEnvelope::Result(value) => {
                assert_eq!(value["conversations"], serde_json::json!([]));
            }
            JsonRpcResponseEnvelope::Error(error) => panic!("unexpected error: {error:?}"),
        }
        writer.await.expect("writer");
        reader_task.await.expect("reader").expect("reader ok");
    }
}
