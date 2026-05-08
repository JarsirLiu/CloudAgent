use crate::node::conversation_registry::ConversationRegistry;
use crate::node::worker_manager::{NodeEvent, WorkerManager};
use agent_core::conversation::ConversationSummary;
use agent_protocol::{
    AppClientCommand, AppClientCommandEnvelope, AppServerMessage, AppServerMessageEnvelope,
    AppServerNotification, JsonRpcMessage,
};
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast};

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
    let workers = WorkerManager::new(worker_program);
    let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        tracing::debug!("accepted local node client from {peer_addr}");
        let workers = workers.clone();
        let conversations = conversations.clone();
        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();
            if let Err(error) =
                run_connection(BufReader::new(reader), writer, workers, conversations).await
            {
                tracing::warn!("local node connection failed: {error}");
            }
        });
    }
}

async fn run_connection<R, W>(
    reader: R,
    mut writer: W,
    workers: WorkerManager,
    conversations: Arc<Mutex<ConversationRegistry>>,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut input_lines = reader.lines();
    let mut active_conversation_id = "default".to_string();
    let mut active_subscription: Option<broadcast::Receiver<NodeEvent>> = None;

    loop {
        if let Some(subscription) = active_subscription.as_mut() {
            tokio::select! {
                maybe_line = input_lines.next_line() => {
                    match maybe_line.context("failed to read local node command line")? {
                        Some(line) => {
                            if !handle_command_line(
                                &line,
                                &mut active_conversation_id,
                                &workers,
                                &conversations,
                                &mut writer,
                                &mut active_subscription,
                            )
                            .await? {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                maybe_event = subscription.recv() => {
                    match maybe_event {
                        Ok(event) => write_node_event(&mut writer, event).await?,
                        Err(broadcast::error::RecvError::Closed) => {
                            active_subscription = None;
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            write_node_event(&mut writer, NodeEvent::Diagnostic {
                                conversation_id: active_conversation_id.clone(),
                                message: format!("local node subscriber lagged; skipped {skipped} events"),
                                is_error: false,
                            }).await?;
                        }
                    }
                }
            }
        } else {
            match input_lines
                .next_line()
                .await
                .context("failed to read local node command line")?
            {
                Some(line) => {
                    if !handle_command_line(
                        &line,
                        &mut active_conversation_id,
                        &workers,
                        &conversations,
                        &mut writer,
                        &mut active_subscription,
                    )
                    .await?
                    {
                        break;
                    }
                }
                None => break,
            }
        }
    }

    Ok(())
}

async fn handle_command_line<W>(
    line: &str,
    active_conversation_id: &mut String,
    workers: &WorkerManager,
    conversations: &Arc<Mutex<ConversationRegistry>>,
    writer: &mut W,
    active_subscription: &mut Option<broadcast::Receiver<NodeEvent>>,
) -> Result<bool>
where
    W: AsyncWrite + Unpin,
{
    let rpc: JsonRpcMessage =
        serde_json::from_str(line).context("failed to parse local node jsonrpc command")?;
    let envelope = AppClientCommandEnvelope::try_from(rpc)?;
    if matches!(envelope.command, AppClientCommand::Exit) {
        return Ok(false);
    }

    if let Some(message) =
        conversation_list_response(&envelope.command, active_conversation_id, conversations).await
    {
        write_app_server_message(writer, message).await?;
        return Ok(true);
    }

    let target_conversation =
        target_conversation_id(active_conversation_id, conversations, &envelope.command).await;
    *active_subscription = Some(workers.subscribe(&target_conversation).await?);
    workers
        .send_command(&target_conversation, envelope.command)
        .await?;
    Ok(true)
}

async fn conversation_list_response(
    command: &AppClientCommand,
    active_conversation_id: &str,
    conversations: &Arc<Mutex<ConversationRegistry>>,
) -> Option<AppServerMessage> {
    if !matches!(command, AppClientCommand::ListConversations) {
        return None;
    }
    let summaries: Vec<ConversationSummary> = conversations.lock().await.summaries();
    Some(AppServerMessage::Notification(
        AppServerNotification::ConversationList {
            conversation_id: active_conversation_id.to_string(),
            conversations: summaries,
        },
    ))
}

async fn write_node_event<W>(writer: &mut W, event: NodeEvent) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message = match event {
        NodeEvent::Message { message } => message,
        NodeEvent::Diagnostic {
            conversation_id,
            message,
            is_error,
        } => AppServerMessage::Notification(if is_error {
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
    };
    write_app_server_message(writer, message).await
}

async fn write_app_server_message<W>(writer: &mut W, message: AppServerMessage) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let envelope = AppServerMessageEnvelope {
        message,
        event_seq: None,
    };
    let payload = serde_json::to_string(&JsonRpcMessage::from(envelope))?;
    writer.write_all(payload.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn target_conversation_id(
    active_conversation_id: &mut String,
    conversations: &Arc<Mutex<ConversationRegistry>>,
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
            let mut registry = conversations.lock().await;
            registry.touch(conversation_id);
            if let AppClientCommand::SetConversationTitle { title, .. } = command {
                registry.set_title(conversation_id, title.clone());
            }
            *active_conversation_id = conversation_id.clone();
            conversation_id.clone()
        }
        AppClientCommand::ListConversations | AppClientCommand::Exit => {
            active_conversation_id.to_string()
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
    use super::{arg_value, conversation_list_response, target_conversation_id};
    use crate::node::conversation_registry::ConversationRegistry;
    use agent_protocol::{AppClientCommand, AppServerMessage, AppServerNotification};
    use std::ffi::OsString;
    use std::sync::Arc;
    use tokio::sync::Mutex;

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
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));
            let mut active = "conversation-1".to_string();
            assert_eq!(
                target_conversation_id(
                    &mut active,
                    &conversations,
                    &AppClientCommand::ListConversations,
                )
                .await,
                "conversation-1"
            );
        });
    }

    #[test]
    fn switch_conversation_updates_active_conversation() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));
            let mut active = "conversation-1".to_string();
            let command = AppClientCommand::SwitchConversation {
                conversation_id: "conversation-2".to_string(),
            };

            assert_eq!(
                target_conversation_id(&mut active, &conversations, &command).await,
                "conversation-2"
            );
            assert_eq!(active, "conversation-2");
            assert_eq!(conversations.lock().await.summaries().len(), 1);
        });
    }

    #[test]
    fn list_conversations_uses_node_shared_registry() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let conversations = Arc::new(Mutex::new(ConversationRegistry::default()));
            {
                let mut registry = conversations.lock().await;
                registry.touch("conversation-1");
                registry.set_title("conversation-1", "Alpha".to_string());
            }

            let message = conversation_list_response(
                &AppClientCommand::ListConversations,
                "conversation-1",
                &conversations,
            )
            .await
            .expect("conversation list message");

            match message {
                AppServerMessage::Notification(AppServerNotification::ConversationList {
                    conversation_id,
                    conversations,
                }) => {
                    assert_eq!(conversation_id, "conversation-1");
                    assert_eq!(conversations.len(), 1);
                    assert_eq!(conversations[0].title.as_deref(), Some("Alpha"));
                }
                other => panic!("unexpected message: {other:?}"),
            }
        });
    }
}
