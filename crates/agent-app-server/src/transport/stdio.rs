use agent_protocol::{AppClientCommandEnvelope, AppServerMessageEnvelope, JsonRpcMessage};
use anyhow::{Context, Result};
use std::collections::HashMap;
use tokio::io::{self, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
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
    write_events_to(&mut stdout, &mut rx).await
}

async fn write_events_to<W>(
    writer: &mut W,
    rx: &mut mpsc::UnboundedReceiver<AppServerMessageEnvelope>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut last_seq_by_conversation: HashMap<String, u64> = HashMap::new();
    while let Some(event) = rx.recv().await {
        if let (Some(conversation_id), Some(event_seq)) = (event.message.conversation_id(), event.event_seq)
        {
            let last_seq = last_seq_by_conversation
                .entry(conversation_id.to_string())
                .or_insert(0);
            if event_seq <= *last_seq {
                continue;
            }
            *last_seq = event_seq;
        }
        let payload = serde_json::to_string(&JsonRpcMessage::from(event))?;
        writer.write_all(payload.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{AppServerMessage, AppServerNotification};
    use tokio::io::{AsyncBufReadExt, BufReader, duplex};

    #[tokio::test]
    async fn write_events_dedupes_replayed_event_seq_per_conversation() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: "first".to_string(),
            }),
            event_seq: Some(1),
        })
        .expect("send first");
        tx.send(AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: "duplicate".to_string(),
            }),
            event_seq: Some(1),
        })
        .expect("send duplicate");
        tx.send(AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: "second".to_string(),
            }),
            event_seq: Some(2),
        })
        .expect("send second");
        drop(tx);

        let (mut write_side, read_side) = duplex(4096);
        write_events_to(&mut write_side, &mut rx)
            .await
            .expect("write events");
        drop(write_side);

        let mut reader = BufReader::new(read_side).lines();
        let mut lines = Vec::new();
        while let Some(line) = reader.next_line().await.expect("read line") {
            lines.push(line);
        }
        assert_eq!(lines.len(), 2);
    }
}
