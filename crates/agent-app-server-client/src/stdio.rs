use crate::{AppServerEvent, DEFAULT_EVENT_CHANNEL_CAPACITY, forward_event};
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessageEnvelope, JsonRpcMessage, RequestId,
};
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

#[derive(Clone, Debug)]
pub struct StdioClientConfig {
    pub program: OsString,
    pub args: Vec<OsString>,
}

pub struct StdioAppServerClient {
    command_tx: mpsc::UnboundedSender<AppClientCommand>,
    event_rx: mpsc::Receiver<AppServerEvent>,
    child: Arc<Mutex<Child>>,
    reader_task: JoinHandle<Result<()>>,
}

impl StdioAppServerClient {
    pub async fn spawn(config: StdioClientConfig) -> Result<Self> {
        let mut command = Command::new(&config.program);
        command.args(&config.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::null());
        command.kill_on_drop(true);

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

        tokio::spawn(write_commands(stdin, command_rx, request_counter));
        let reader_task = tokio::spawn(read_events(stdout, event_tx));

        Ok(Self {
            command_tx,
            event_rx,
            child: Arc::new(Mutex::new(child)),
            reader_task,
        })
    }

    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .map_err(|_| anyhow!("stdio app-server command channel is closed"))
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        self.event_rx.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        let StdioAppServerClient {
            command_tx,
            event_rx: _,
            child,
            reader_task,
        } = self;

        let _ = command_tx.send(AppClientCommand::Exit);
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

async fn write_commands(
    mut stdin: ChildStdin,
    mut command_rx: mpsc::UnboundedReceiver<AppClientCommand>,
    request_counter: Arc<AtomicI64>,
) -> Result<()> {
    write_commands_to(&mut stdin, &mut command_rx, request_counter).await
}

async fn write_commands_to<W>(
    writer: &mut W,
    command_rx: &mut mpsc::UnboundedReceiver<AppClientCommand>,
    request_counter: Arc<AtomicI64>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(command) = command_rx.recv().await {
        let envelope = AppClientCommandEnvelope {
            request_id: RequestId::Integer(request_counter.fetch_add(1, Ordering::Relaxed)),
            command,
        };
        let payload = serde_json::to_string(&JsonRpcMessage::from(envelope))?;
        writer.write_all(payload.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }
    Ok(())
}

async fn read_events(
    stdout: tokio::process::ChildStdout,
    event_tx: mpsc::Sender<AppServerEvent>,
) -> Result<()> {
    read_events_from(BufReader::new(stdout), event_tx).await
}

async fn read_events_from<R>(reader: R, event_tx: mpsc::Sender<AppServerEvent>) -> Result<()>
where
    R: AsyncBufRead + Unpin,
{
    let mut lines = reader.lines();
    let mut skipped_events = 0usize;
    let mut last_seq_by_conversation: HashMap<String, u64> = HashMap::new();

    while let Some(line) = lines.next_line().await? {
        let message: JsonRpcMessage =
            serde_json::from_str(&line).context("failed to parse stdio app-server event")?;
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

    let _ = forward_event(
        &event_tx,
        &mut skipped_events,
        AppServerEvent::Disconnected {
            message: "stdio app server closed".to_string(),
        },
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, ApprovalPolicy,
        CommandApprovalRequest, JsonRpcMessage, PermissionProfile, ServerRequest, TurnPolicy,
        UserTurnInput,
    };
    use tokio::io::duplex;

    #[tokio::test]
    async fn write_commands_serializes_jsonrpc_lines() {
        let (mut write_side, read_side) = duplex(4096);
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let counter = Arc::new(AtomicI64::new(7));

        command_tx
            .send(AppClientCommand::SubmitTurn(UserTurnInput {
                conversation_id: "default".to_string(),
                content: "hello".to_string(),
                turn_policy: TurnPolicy {
                    permission_profile: PermissionProfile::ReadOnly,
                    approval_policy: ApprovalPolicy::OnRequest,
                },
            }))
            .expect("queue command");
        drop(command_tx);

        write_commands_to(&mut write_side, &mut command_rx, counter)
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
                assert_eq!(input.content, "hello");
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
                request: ServerRequest::CommandApproval {
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

        read_events_from(BufReader::new(read_side), event_tx)
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
                    request: ServerRequest::CommandApproval { request },
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

        read_events_from(BufReader::new(read_side), event_tx)
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
}
