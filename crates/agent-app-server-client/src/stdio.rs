use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerMessageEnvelope,
    JsonRpcMessage, RequestId,
};
use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};

#[derive(Clone, Debug)]
pub struct StdioClientConfig {
    pub program: OsString,
    pub args: Vec<OsString>,
}

pub struct StdioAppServerClient {
    command_tx: mpsc::UnboundedSender<AppClientCommand>,
    event_rx: mpsc::UnboundedReceiver<AppServerMessage>,
    child: Arc<Mutex<Child>>,
}

impl StdioAppServerClient {
    pub async fn spawn(config: StdioClientConfig) -> Result<Self> {
        let mut command = Command::new(&config.program);
        command.args(&config.args);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::inherit());

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
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let request_counter = Arc::new(AtomicI64::new(1));

        tokio::spawn(write_commands(stdin, command_rx, request_counter));
        tokio::spawn(read_events(stdout, event_tx));

        Ok(Self {
            command_tx,
            event_rx,
            child: Arc::new(Mutex::new(child)),
        })
    }

    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .map_err(|_| anyhow!("stdio app-server command channel is closed"))
    }

    pub async fn next_message(&mut self) -> Option<AppServerMessage> {
        self.event_rx.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        let mut child = self.child.lock().await;
        if child.try_wait()?.is_none() {
            child.kill().await.ok();
        }
        Ok(())
    }
}

async fn write_commands(
    mut stdin: ChildStdin,
    mut command_rx: mpsc::UnboundedReceiver<AppClientCommand>,
    request_counter: Arc<AtomicI64>,
) -> Result<()> {
    while let Some(command) = command_rx.recv().await {
        let envelope = AppClientCommandEnvelope {
            request_id: RequestId::Integer(request_counter.fetch_add(1, Ordering::Relaxed)),
            command,
        };
        let payload = serde_json::to_string(&JsonRpcMessage::from(envelope))?;
        stdin.write_all(payload.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
    }
    Ok(())
}

async fn read_events(
    stdout: tokio::process::ChildStdout,
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
) -> Result<()> {
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let message: JsonRpcMessage =
            serde_json::from_str(&line).context("failed to parse stdio app-server event")?;
        let envelope = AppServerMessageEnvelope::try_from(message)?;

        if event_tx.send(envelope.message).is_err() {
            break;
        }
    }

    Ok(())
}
