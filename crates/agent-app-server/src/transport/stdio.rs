use agent_protocol::{AppClientCommandEnvelope, AppServerMessageEnvelope, JsonRpcMessage};
use anyhow::{Context, Result};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub async fn read_commands(tx: mpsc::UnboundedSender<AppClientCommandEnvelope>) -> Result<()> {
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();

    while let Some(line) = lines.next_line().await? {
        let message: JsonRpcMessage = serde_json::from_str(&line)
            .with_context(|| "failed to parse stdio app-server request")?;
        let envelope = AppClientCommandEnvelope::try_from(message)?;
        if tx.send(envelope).is_err() {
            break;
        }
    }

    Ok(())
}

pub async fn write_events(mut rx: mpsc::UnboundedReceiver<AppServerMessageEnvelope>) -> Result<()> {
    let mut stdout = io::stdout();
    while let Some(event) = rx.recv().await {
        let payload = serde_json::to_string(&JsonRpcMessage::from(event))?;
        stdout.write_all(payload.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}
