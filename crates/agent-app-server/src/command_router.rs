use crate::conversation_listener::ConversationListenerHandle;
use crate::conversation_listener::start_conversation_listener;
use crate::conversation_subscriptions::ConversationSubscriptions;
use crate::server_request_coordinator::ServerRequestCoordinator;
use agent_core::ConversationTurn;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest,
    SequencedAppServerMessage, ServerRequest, ServerRequestDecision,
};
use agent_runtime::AgentRuntime;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

pub(crate) struct ServerState {
    subscriptions: ConversationSubscriptions,
    server_requests: ServerRequestCoordinator,
    turn_tasks: Vec<JoinHandle<()>>,
    active_listeners: HashMap<String, ConversationListenerHandle>,
    message_log: Vec<SequencedAppServerMessage>,
    next_message_sequence: u64,
}

impl ServerState {
    pub(crate) fn new(default_conversation_id: String) -> Self {
        Self {
            subscriptions: ConversationSubscriptions::new(default_conversation_id),
            server_requests: ServerRequestCoordinator::new(),
            turn_tasks: Vec::new(),
            active_listeners: HashMap::new(),
            message_log: Vec::new(),
            next_message_sequence: 1,
        }
    }

    pub(crate) fn track_turn_task(&mut self, task: JoinHandle<()>) {
        self.turn_tasks.retain(|task| !task.is_finished());
        self.turn_tasks.push(task);
    }

    pub(crate) fn take_turn_tasks(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.turn_tasks)
    }

    pub(crate) fn set_active_listener(
        &mut self,
        conversation_id: String,
        listener: ConversationListenerHandle,
    ) {
        self.active_listeners.insert(conversation_id, listener);
    }

    pub(crate) fn clear_active_listener(&mut self, conversation_id: &str) {
        self.active_listeners.remove(conversation_id);
    }

    pub(crate) fn active_listener(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationListenerHandle> {
        self.active_listeners.get(conversation_id).cloned()
    }

    pub(crate) fn record_message(&mut self, message: AppServerMessage) {
        if !should_record_message(&message) {
            return;
        }
        let sequence = self.next_message_sequence;
        self.next_message_sequence = self.next_message_sequence.saturating_add(1);
        self.message_log
            .push(SequencedAppServerMessage { sequence, message });
    }

    pub(crate) fn messages_after(
        &self,
        conversation_id: &str,
        after_sequence: u64,
    ) -> Vec<SequencedAppServerMessage> {
        self.message_log
            .iter()
            .filter(|entry| {
                entry.sequence > after_sequence
                    && entry.message.conversation_id() == Some(conversation_id)
            })
            .cloned()
            .collect()
    }
}

fn should_record_message(message: &AppServerMessage) -> bool {
    match message {
        AppServerMessage::Request(_) => true,
        AppServerMessage::Notification(notification) => !matches!(
            notification,
            AppServerNotification::ConversationStatus { .. }
                | AppServerNotification::ConversationHistory { .. }
                | AppServerNotification::ConversationNotifications { .. }
                | AppServerNotification::ConversationSubscriptionChanged { .. }
                | AppServerNotification::Info { .. }
        ),
    }
}

async fn await_tracked_turn_tasks(state: &Arc<Mutex<ServerState>>) {
    let tasks = {
        let mut guard = state.lock().await;
        guard.take_turn_tasks()
    };
    for task in tasks {
        let _ = task.await;
    }
}

#[derive(Clone)]
pub(crate) struct TurnSpawnDependencies {
    pub(crate) event_tx: mpsc::UnboundedSender<AppServerMessage>,
    pub(crate) state: Arc<Mutex<ServerState>>,
    pub(crate) auto_approve: bool,
    pub(crate) auto_approve_reason: Option<String>,
}

pub(crate) async fn handle_command(
    runtime: Arc<AgentRuntime>,
    command: AppClientCommand,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    match command {
        AppClientCommand::SubmitTurn(input) => {
            await_tracked_turn_tasks(&state).await;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::FrontendStateChanged {
                    conversation_id: input.conversation_id.clone(),
                    mode: agent_protocol::FrontendMode::Running,
                },
            )
            .await;
            let task = spawn_turn(
                runtime,
                input.conversation_id,
                input.content,
                TurnSpawnDependencies {
                    event_tx: event_tx.clone(),
                    state: state.clone(),
                    auto_approve,
                    auto_approve_reason,
                },
            );
            state.lock().await.track_turn_task(task);
        }
        AppClientCommand::InterruptTurn { conversation_id } => {
            let interrupted = runtime.interrupt_conversation(&conversation_id).await;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::Info {
                    conversation_id,
                    message: if interrupted {
                        "interrupt requested".to_string()
                    } else {
                        "no active turn".to_string()
                    },
                },
            )
            .await;
        }
        AppClientCommand::RequestConversationStatus { conversation_id } => {
            let snapshot = runtime.conversation_status(&conversation_id).await?;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::ConversationStatus {
                    conversation_id,
                    snapshot,
                },
            )
            .await;
        }
        AppClientCommand::RequestConversationHistory { conversation_id } => {
            let active_listener = {
                let state = state.lock().await;
                state.active_listener(&conversation_id)
            };
            let active_turn = match active_listener {
                Some(listener) => listener.active_turn_snapshot().await,
                None => None,
            };
            let mut turns = runtime.build_turns_from_rollout(&conversation_id).await?;
            merge_active_turn(&mut turns, active_turn);
            send_notification(
                event_tx,
                &state,
                AppServerNotification::ConversationHistory {
                    conversation_id,
                    turns,
                },
            )
            .await;
        }
        AppClientCommand::RequestConversationNotifications {
            conversation_id,
            after_sequence,
        } => {
            let messages = {
                let state = state.lock().await;
                state.messages_after(&conversation_id, after_sequence)
            };
            send_ephemeral_notification(
                event_tx,
                &state,
                AppServerNotification::ConversationNotifications {
                    conversation_id,
                    from_sequence: after_sequence.saturating_add(1),
                    messages,
                },
            )
            .await;
        }
        AppClientCommand::ResetConversation { conversation_id } => {
            runtime.reset_conversation(&conversation_id).await?;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::Info {
                    conversation_id,
                    message: "conversation reset".to_string(),
                },
            )
            .await;
        }
        AppClientCommand::SubscribeConversation { conversation_id } => {
            {
                let mut state = state.lock().await;
                state.subscriptions.subscribe(conversation_id.clone());
            }
            send_notification(
                event_tx,
                &state,
                AppServerNotification::ConversationSubscriptionChanged {
                    conversation_id,
                    subscribed: true,
                },
            )
            .await;
        }
        AppClientCommand::UnsubscribeConversation { conversation_id } => {
            {
                let mut state = state.lock().await;
                state.subscriptions.unsubscribe(&conversation_id);
            }
            let _ = event_tx.send(AppServerMessage::Notification(
                AppServerNotification::ConversationSubscriptionChanged {
                    conversation_id,
                    subscribed: false,
                },
            ));
        }
        AppClientCommand::ResolveServerRequest {
            conversation_id,
            request_id,
            approved,
            reason,
        } => {
            let mut state_guard = state.lock().await;
            let resolved = state_guard.server_requests.resolve(
                &request_id,
                ServerRequestDecision {
                    approved,
                    reason: reason.clone(),
                },
            );
            drop(state_guard);
            if let Some((turn_id, request, decision)) = resolved {
                runtime
                    .resolve_pending_request(&conversation_id, &request_id)
                    .await;
                send_notification(
                    event_tx,
                    &state,
                    AppServerNotification::ServerRequestResolved {
                        conversation_id,
                        turn_id,
                        request_id,
                        request,
                        decision,
                    },
                )
                .await;
            }
        }
        AppClientCommand::Exit => {}
    }

    Ok(())
}

fn spawn_turn(
    runtime: Arc<AgentRuntime>,
    conversation_id: String,
    user_input: String,
    deps: TurnSpawnDependencies,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let finish_events = deps.event_tx.clone();
        let state_for_finish = deps.state.clone();
        let conversation_id_for_server_request = conversation_id.clone();
        let active_turn_id = Arc::new(StdMutex::new(None::<String>));
        let active_turn_id_for_events = active_turn_id.clone();
        let runtime_for_requests = runtime.clone();
        let (listener, listener_task) = start_conversation_listener(
            conversation_id.clone(),
            deps.event_tx.clone(),
            deps.state.clone(),
        );
        {
            let mut state = deps.state.lock().await;
            state.set_active_listener(conversation_id.clone(), listener.clone());
        }
        let listener_for_events = listener.clone();

        let result = runtime
            .chat_with_approval_and_events(
                &conversation_id,
                &user_input,
                move |event| {
                    let event = event.clone();
                    let active_turn_id = active_turn_id_for_events.clone();
                    if let agent_protocol::EventMsg::TurnStarted { turn_id, .. } = &event
                        && let Ok(mut active) = active_turn_id.lock()
                    {
                        *active = Some(turn_id.clone());
                    }
                    listener_for_events.project_event(event);
                },
                move |request: ServerRequest| {
                    let event_tx = deps.event_tx.clone();
                    let state = deps.state.clone();
                    let conversation_id = conversation_id_for_server_request.clone();
                    let auto_approve_reason = deps.auto_approve_reason.clone();
                    let runtime = runtime_for_requests.clone();
                    async move {
                        if deps.auto_approve {
                            return Ok(ServerRequestDecision {
                                approved: true,
                                reason: auto_approve_reason
                                    .clone()
                                    .or_else(|| Some("auto-approved by app server".to_string())),
                            });
                        }

                        let request_id = {
                            let state_guard = state.lock().await;
                            state_guard.server_requests.next_request_id()
                        };
                        let (reply_tx, reply_rx) = oneshot::channel();
                        let turn_id = match &request {
                            ServerRequest::ToolApproval { request } => request.turn_id.clone(),
                        };
                        {
                            let mut state_guard = state.lock().await;
                            state_guard.server_requests.insert_pending(
                                request_id.clone(),
                                turn_id,
                                request.clone(),
                                reply_tx,
                            );
                        }
                        runtime
                            .register_pending_request(
                                &conversation_id,
                                request_id.clone(),
                                request.clone(),
                            )
                            .await;
                        send_request(
                            &event_tx,
                            &state,
                            AppServerRequest::ServerRequest {
                                request_id,
                                conversation_id,
                                request,
                            },
                        )
                        .await;
                        reply_rx
                            .await
                            .map_err(|_| anyhow!("server request response channel closed"))
                    }
                },
            )
            .await;

        match result {
            Ok(output) => {
                listener.finish_turn(output.state).await;
            }
            Err(error) => {
                let maybe_turn_id = active_turn_id.lock().ok().and_then(|guard| guard.clone());
                if let Some(turn_id) = maybe_turn_id {
                    send_notification(
                        &finish_events,
                        &state_for_finish,
                        AppServerNotification::TurnFailed {
                            conversation_id: conversation_id.clone(),
                            turn_id,
                            error: format!("{error:#}"),
                        },
                    )
                    .await;
                } else {
                    send_notification(
                        &finish_events,
                        &state_for_finish,
                        AppServerNotification::Error {
                            conversation_id: conversation_id.clone(),
                            message: format!("turn failed before start: {error:#}"),
                        },
                    )
                    .await;
                }
                send_notification(
                    &finish_events,
                    &state_for_finish,
                    AppServerNotification::FrontendStateChanged {
                        conversation_id: conversation_id.clone(),
                        mode: agent_protocol::FrontendMode::Idle,
                    },
                )
                .await;
                listener
                    .finish_turn(agent_protocol::TurnState::Failed)
                    .await;
            }
        }
        let _ = listener_task.await;
        let mut state = state_for_finish.lock().await;
        state.clear_active_listener(&conversation_id);
    })
}

fn merge_active_turn(turns: &mut Vec<ConversationTurn>, active_turn: Option<ConversationTurn>) {
    let Some(active_turn) = active_turn else {
        return;
    };
    if let Some(existing) = turns.iter_mut().find(|turn| turn.id == active_turn.id) {
        *existing = active_turn;
    } else {
        turns.push(active_turn);
    }
}

#[cfg(test)]
mod tests {
    use super::{ServerState, merge_active_turn};
    use agent_core::{ConversationTurn, TranscriptItem};
    use agent_protocol::{AppServerMessage, AppServerNotification, TurnState};

    #[test]
    fn active_turn_snapshot_replaces_matching_rollout_turn() {
        let mut turns = vec![turn("turn-1", "old")];

        merge_active_turn(&mut turns, Some(turn("turn-1", "live")));

        assert_eq!(turns.len(), 1);
        assert!(matches!(
            &turns[0].items[..],
            [TranscriptItem::AgentMessage { text, .. }] if text == "live"
        ));
    }

    #[test]
    fn active_turn_snapshot_appends_when_rollout_has_no_matching_turn() {
        let mut turns = vec![turn("turn-1", "old")];

        merge_active_turn(&mut turns, Some(turn("turn-2", "live")));

        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].id, "turn-2");
    }

    #[test]
    fn notification_log_replays_by_conversation_and_sequence() {
        let mut state = ServerState::new("default".to_string());
        state.record_message(AppServerMessage::Notification(
            AppServerNotification::TurnStarted {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
            },
        ));
        state.record_message(AppServerMessage::Notification(
            AppServerNotification::TurnStarted {
                conversation_id: "other".to_string(),
                turn_id: "turn-2".to_string(),
            },
        ));
        state.record_message(AppServerMessage::Notification(
            AppServerNotification::TurnCompleted {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
            },
        ));

        let replay = state.messages_after("default", 1);

        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].sequence, 3);
        assert!(matches!(
            replay[0].message,
            AppServerMessage::Notification(AppServerNotification::TurnCompleted { .. })
        ));
    }

    #[test]
    fn notification_log_skips_snapshot_notifications() {
        let mut state = ServerState::new("default".to_string());
        state.record_message(AppServerMessage::Notification(
            AppServerNotification::ConversationHistory {
                conversation_id: "default".to_string(),
                turns: Vec::new(),
            },
        ));

        assert!(state.messages_after("default", 0).is_empty());
    }

    fn turn(id: &str, text: &str) -> ConversationTurn {
        ConversationTurn {
            id: id.to_string(),
            state: TurnState::Running,
            items: vec![TranscriptItem::AgentMessage {
                id: format!("assistant:{id}"),
                text: text.to_string(),
            }],
            rollout_start_index: 0,
            rollout_end_index: 0,
        }
    }
}

pub(crate) async fn send_notification(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    notification: AppServerNotification,
) {
    let subscribed = {
        let state = state.lock().await;
        state
            .subscriptions
            .is_subscribed(notification.conversation_id())
    };
    let message = AppServerMessage::Notification(notification);
    {
        let mut state = state.lock().await;
        state.record_message(message.clone());
    }
    if subscribed {
        let _ = event_tx.send(message);
    }
}

pub(crate) async fn send_ephemeral_notification(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    notification: AppServerNotification,
) {
    let subscribed = {
        let state = state.lock().await;
        state
            .subscriptions
            .is_subscribed(notification.conversation_id())
    };
    if subscribed {
        let _ = event_tx.send(AppServerMessage::Notification(notification));
    }
}

async fn send_request(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    request: AppServerRequest,
) {
    let subscribed = {
        let state = state.lock().await;
        state.subscriptions.is_subscribed(request.conversation_id())
    };
    let message = AppServerMessage::Request(request);
    {
        let mut state = state.lock().await;
        state.record_message(message.clone());
    }
    if subscribed {
        let _ = event_tx.send(message);
    }
}
