use crate::node::conversation_registry::ConversationRegistry;
use crate::node::worker_manager::{NodeEvent, WorkerManager};
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerMessageEnvelope,
    AppServerNotification, JsonRpcMessage,
};
use anyhow::{Context, Result};
use std::ffi::OsString;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub(crate) async fn run_resident_node(args: &[OsString]) -> Result<()> {
    let listen_address = arg_value(args, "--listen")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| default_listen_address().to_string());
    let worker_program = arg_value(args, "--worker-bin")
        .or_else(|| std::env::var_os("CLOUDAGENT_WORKER_BIN"))
        .unwrap_or_else(default_worker_bin);

    let listener = TcpListener::bind(&listen_address)
        .await
        .with_context(|| format!("failed to bind local node listener on {listen_address}"))?;
    tracing::info!("gatewayd local node listening on {listen_address}");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        tracing::debug!("accepted local node client from {peer_addr}");
        let worker_program = worker_program.clone();
        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();
            if let Err(error) = run_connection(BufReader::new(reader), writer, worker_program).await
            {
                tracing::warn!("local node connection failed: {error}");
            }
        });
    }
}

async fn run_connection<R, W>(reader: R, mut writer: W, worker_program: OsString) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut input_lines = reader.lines();
    let (event_tx, mut event_rx) = mpsc::channel(128);

    let mut registry = ConversationRegistry::new("default".to_string());
    let mut workers = WorkerManager::new(worker_program);

    loop {
        tokio::select! {
            maybe_line = input_lines.next_line() => {
                match maybe_line.context("failed to read local node command line")? {
                    Some(line) => {
                        let rpc: JsonRpcMessage = serde_json::from_str(&line)
                            .context("failed to parse local node jsonrpc command")?;
                        let envelope = AppClientCommandEnvelope::try_from(rpc)?;
                        if matches!(envelope.command, AppClientCommand::Exit) {
                            break;
                        }
                        let target_conversation = target_conversation_id(&mut registry, &envelope.command);
                        workers
                            .send_command(&target_conversation, envelope.command, event_tx.clone())
                            .await?;
                    }
                    None => break,
                }
            }
            maybe_event = event_rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        let envelope = match event {
                            NodeEvent::Message { message } => AppServerMessageEnvelope {
                                message,
                                event_seq: None,
                            },
                            NodeEvent::Diagnostic {
                                conversation_id,
                                message,
                                is_error,
                            } => AppServerMessageEnvelope {
                                message: AppServerMessage::Notification(if is_error {
                                    AppServerNotification::Error {
                                        conversation_id,
                                        message,
                                    }
                                } else {
                                    AppServerNotification::Info {
                                        conversation_id,
                                        message,
                                    }
                                }),
                                event_seq: None,
                            },
                        };
                        let payload = serde_json::to_string(&JsonRpcMessage::from(envelope))?;
                        writer.write_all(payload.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await?;
                    }
                    None => break,
                }
            }
        }
    }

    drop(event_tx);
    workers.shutdown().await?;
    Ok(())
}

fn target_conversation_id(
    registry: &mut ConversationRegistry,
    command: &AppClientCommand,
) -> String {
    match command {
        AppClientCommand::SwitchConversation { conversation_id }
        | AppClientCommand::CreateConversation { conversation_id }
        | AppClientCommand::SubmitTurn(agent_protocol::UserTurnInput {
            conversation_id, ..
        })
        | AppClientCommand::ResolveServerRequest {
            conversation_id, ..
        }
        | AppClientCommand::InterruptTurn { conversation_id }
        | AppClientCommand::CompactConversation { conversation_id }
        | AppClientCommand::ResetConversation { conversation_id }
        | AppClientCommand::RequestConversationStatus { conversation_id }
        | AppClientCommand::RequestConversationHistory { conversation_id }
        | AppClientCommand::RequestConversationHistoryPage {
            conversation_id, ..
        }
        | AppClientCommand::SetConversationTitle {
            conversation_id, ..
        }
        | AppClientCommand::ArchiveConversation { conversation_id }
        | AppClientCommand::DeleteConversation { conversation_id }
        | AppClientCommand::SubscribeConversation { conversation_id }
        | AppClientCommand::UnsubscribeConversation { conversation_id } => {
            registry.set_active_conversation(conversation_id.clone());
            conversation_id.clone()
        }
        AppClientCommand::ListConversations | AppClientCommand::Exit => {
            registry.active_conversation_id().to_string()
        }
    }
}

fn default_listen_address() -> &'static str {
    "127.0.0.1:47070"
}

fn default_worker_bin() -> OsString {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(exe_name("agentd"))))
        .map(|path| path.into_os_string())
        .unwrap_or_else(|| OsString::from(exe_name("agentd")))
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn arg_value(args: &[OsString], name: &str) -> Option<OsString> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

#[cfg(test)]
mod tests {
    use super::{arg_value, target_conversation_id};
    use crate::node::conversation_registry::ConversationRegistry;
    use agent_protocol::AppClientCommand;
    use std::ffi::OsString;

    #[test]
    fn parses_serve_flag_values() {
        let args = vec![
            OsString::from("--listen"),
            OsString::from("127.0.0.1:47070"),
            OsString::from("--worker-bin"),
            OsString::from("agentd.exe"),
        ];
        assert_eq!(
            arg_value(&args, "--listen"),
            Some(OsString::from("127.0.0.1:47070"))
        );
        assert_eq!(
            arg_value(&args, "--worker-bin"),
            Some(OsString::from("agentd.exe"))
        );
    }

    #[test]
    fn list_conversations_routes_to_active_conversation() {
        let mut registry = ConversationRegistry::new("conversation-1".to_string());
        assert_eq!(
            target_conversation_id(&mut registry, &AppClientCommand::ListConversations),
            "conversation-1"
        );
    }

    #[test]
    fn switch_conversation_updates_active_conversation() {
        let mut registry = ConversationRegistry::new("conversation-1".to_string());
        let command = AppClientCommand::SwitchConversation {
            conversation_id: "conversation-2".to_string(),
        };

        assert_eq!(
            target_conversation_id(&mut registry, &command),
            "conversation-2"
        );
        assert_eq!(registry.active_conversation_id(), "conversation-2");
    }
}
