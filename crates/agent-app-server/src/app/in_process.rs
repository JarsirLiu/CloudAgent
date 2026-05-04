use crate::routing::command_router::{ServerState, handle_command};
use crate::session::state as session_state;
use agent_protocol::{AppClientCommand, AppServerMessage, AppServerNotification};
use agent_core::AgentHost;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Debug)]
enum ServerMessage {
    Command(AppClientCommand),
    Shutdown { done: oneshot::Sender<()> },
}

pub struct InProcessClientHandle {
    command_tx: mpsc::UnboundedSender<ServerMessage>,
    event_rx: mpsc::UnboundedReceiver<AppServerMessage>,
}

#[derive(Clone)]
pub struct InProcessClientSender {
    command_tx: mpsc::UnboundedSender<ServerMessage>,
}

impl InProcessClientSender {
    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.command_tx
            .send(ServerMessage::Command(command))
            .map_err(|_| anyhow!("in-process app server is closed"))
    }
}

impl InProcessClientHandle {
    pub fn sender(&self) -> InProcessClientSender {
        InProcessClientSender {
            command_tx: self.command_tx.clone(),
        }
    }

    pub async fn next_message(&mut self) -> Option<AppServerMessage> {
        self.event_rx.recv().await
    }

    pub async fn shutdown(self) -> Result<()> {
        let (done_tx, done_rx) = oneshot::channel();
        self.command_tx
            .send(ServerMessage::Shutdown { done: done_tx })
            .map_err(|_| anyhow!("in-process app server is closed"))?;
        let _ = done_rx.await;
        Ok(())
    }
}

pub struct InProcessServer;

pub fn start_in_process(
    runtime: Arc<AgentHost>,
    conversation_id: String,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> InProcessClientHandle {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AppServerMessage>();
    let state = Arc::new(Mutex::new(ServerState::new(conversation_id.clone())));

    tokio::spawn(async move {
        session_state::hydrate_active_conversation(&runtime, &state).await;
        while let Some(message) = command_rx.recv().await {
            match message {
                ServerMessage::Command(AppClientCommand::Exit) => {
                    let tasks = {
                        let mut guard = state.lock().await;
                        guard.take_all_turn_tasks()
                    };
                    for task in tasks {
                        let _ = task.await;
                    }
                    break;
                }
                ServerMessage::Command(command) => {
                    let command_conversation_id = command.conversation_id().map(str::to_string);
                    let should_mark_active = matches!(command, AppClientCommand::SubmitTurn(_));
                    if handle_command(
                        runtime.clone(),
                        command,
                        &event_tx,
                        state.clone(),
                        auto_approve,
                        auto_approve_reason.clone(),
                    )
                    .await
                    .is_err()
                    {
                        let _ = event_tx.send(AppServerMessage::Notification(
                            AppServerNotification::Error {
                                conversation_id: conversation_id.clone(),
                                message: "command handling failed".to_string(),
                            },
                        ));
                    } else if let (Some(id), true) = (command_conversation_id, should_mark_active) {
                        session_state::persist_active_conversation(&runtime, &id).await;
                    }
                }
                ServerMessage::Shutdown { done } => {
                    let tasks = {
                        let mut guard = state.lock().await;
                        guard.take_all_turn_tasks()
                    };
                    for task in tasks {
                        let _ = task.await;
                    }
                    let _ = done.send(());
                    break;
                }
            }
        }
    });

    InProcessClientHandle {
        command_tx,
        event_rx,
    }
}


