use crate::app::notification::send_notification;
use crate::projection::ConversationNotificationProjector;
use crate::routing::command_router::ServerState;
use agent_core::ConversationTurn;
use agent_core::{EventMsg, TurnState};
use agent_protocol::AppServerMessage;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;

const ACTIVE_TURN_SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub(crate) struct ConversationListenerHandle {
    command_tx: mpsc::UnboundedSender<ConversationListenerCommand>,
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum ConversationListenerCommand {
    ProjectEvent(EventMsg),
    ActiveTurnSnapshot {
        ack: oneshot::Sender<Option<ConversationTurn>>,
    },
    FinishTurn {
        turn_state: TurnState,
        ack: oneshot::Sender<()>,
    },
}

pub(crate) fn start_conversation_listener(
    conversation_id: String,
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
) -> (ConversationListenerHandle, JoinHandle<()>) {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ConversationListenerCommand>();
    let handle = ConversationListenerHandle { command_tx };
    let task = tokio::spawn(async move {
        let mut projector = ConversationNotificationProjector::new(conversation_id);
        while let Some(command) = command_rx.recv().await {
            match command {
                ConversationListenerCommand::ProjectEvent(event) => {
                    for notification in projector.project_turn_event(&event) {
                        send_notification(&event_tx, &state, notification).await;
                    }
                }
                ConversationListenerCommand::ActiveTurnSnapshot { ack } => {
                    let _ = ack.send(projector.active_turn_snapshot());
                }
                ConversationListenerCommand::FinishTurn { turn_state, ack } => {
                    for notification in projector.finish_turn(turn_state) {
                        send_notification(&event_tx, &state, notification).await;
                    }
                    let _ = ack.send(());
                    break;
                }
            }
        }
    });
    (handle, task)
}

impl ConversationListenerHandle {
    pub(crate) fn project_event(&self, event: EventMsg) {
        let _ = self
            .command_tx
            .send(ConversationListenerCommand::ProjectEvent(event));
    }

    pub(crate) async fn active_turn_snapshot(&self) -> Option<ConversationTurn> {
        let (ack, done) = oneshot::channel();
        self.command_tx
            .send(ConversationListenerCommand::ActiveTurnSnapshot { ack })
            .ok()?;
        timeout(ACTIVE_TURN_SNAPSHOT_TIMEOUT, done)
            .await
            .ok()
            .and_then(|result| result.ok())
            .flatten()
    }

    pub(crate) async fn finish_turn(&self, turn_state: TurnState) {
        let (ack, done) = oneshot::channel();
        if self
            .command_tx
            .send(ConversationListenerCommand::FinishTurn { turn_state, ack })
            .is_ok()
        {
            let _ = done.await;
        }
    }
}
