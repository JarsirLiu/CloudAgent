use crate::conversation_subscriptions::ConversationSubscriptions;
use crate::projection::ConversationNotificationProjector;
use crate::server_request_coordinator::ServerRequestCoordinator;
use agent_core::history_entry_from_message;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, ServerRequest,
    ServerRequestDecision,
};
use agent_runtime::AgentRuntime;
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

pub(crate) struct ServerState {
    subscriptions: ConversationSubscriptions,
    server_requests: ServerRequestCoordinator,
    turn_tasks: Vec<JoinHandle<()>>,
}

impl ServerState {
    pub(crate) fn new(default_conversation_id: String) -> Self {
        Self {
            subscriptions: ConversationSubscriptions::new(default_conversation_id),
            server_requests: ServerRequestCoordinator::new(),
            turn_tasks: Vec::new(),
        }
    }

    pub(crate) fn track_turn_task(&mut self, task: JoinHandle<()>) {
        self.turn_tasks.retain(|task| !task.is_finished());
        self.turn_tasks.push(task);
    }

    pub(crate) fn take_turn_tasks(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.turn_tasks)
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
pub(crate) struct SpawnTurnContext {
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
                SpawnTurnContext {
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
            let snapshot = runtime.conversation_history_snapshot(&conversation_id).await?;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::ConversationHistory {
                    conversation_id,
                    messages: snapshot
                        .messages
                        .iter()
                        .map(history_entry_from_message)
                        .collect(),
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
            let resolved = state_guard
                .server_requests
                .resolve(&request_id, ServerRequestDecision { approved, reason: reason.clone() });
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
    ctx: SpawnTurnContext,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let runtime_events = ctx.event_tx.clone();
        let finish_events = ctx.event_tx.clone();
        let state_for_turn = ctx.state.clone();
        let state_for_finish = ctx.state.clone();
        let conversation_id_for_turn = conversation_id.clone();
        let conversation_id_for_server_request = conversation_id.clone();
        let active_turn_id = Arc::new(StdMutex::new(None::<String>));
        let active_turn_id_for_events = active_turn_id.clone();
        let projector = Arc::new(StdMutex::new(ConversationNotificationProjector::new(
            conversation_id_for_turn.clone(),
        )));
        let projector_for_events = projector.clone();
        let (projected_tx, mut projected_rx) =
            mpsc::unbounded_channel::<Vec<AppServerNotification>>();
        let projected_tx_for_events = projected_tx.clone();
        let runtime_events_for_projected = runtime_events.clone();
        let state_for_projected = state_for_turn.clone();
        let runtime_for_requests = runtime.clone();
        let projected_task = tokio::spawn(async move {
            while let Some(notifications) = projected_rx.recv().await {
                for notification in notifications {
                    send_notification(&runtime_events_for_projected, &state_for_projected, notification)
                        .await;
                }
            }
        });

        let result = runtime
            .chat_with_approval_and_events(
                &conversation_id,
                &user_input,
                move |event| {
                    let event = event.clone();
                    let active_turn_id = active_turn_id_for_events.clone();
                    let projected_tx = projected_tx_for_events.clone();
                    if let agent_protocol::TurnEvent::TurnStarted { turn_id, .. } = &event
                        && let Ok(mut active) = active_turn_id.lock()
                    {
                        *active = Some(turn_id.clone());
                    }
                    let notifications = projector_for_events
                        .lock()
                        .ok()
                        .map(|mut projector| projector.project_turn_event(&event))
                        .unwrap_or_default();
                    let _ = projected_tx.send(notifications);
                },
                move |request: ServerRequest| {
                    let event_tx = ctx.event_tx.clone();
                    let state = ctx.state.clone();
                    let conversation_id = conversation_id_for_server_request.clone();
                    let auto_approve_reason = ctx.auto_approve_reason.clone();
                    let runtime = runtime_for_requests.clone();
                    async move {
                        if ctx.auto_approve {
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
                            state_guard
                                .server_requests
                                .insert_pending(
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

        drop(projected_tx);
        let _ = projected_task.await;

        if let Ok(output) = &result {
            let notifications = projector
                .lock()
                .ok()
                .map(|mut projector| projector.finish_turn(output.state.clone()))
                .unwrap_or_default();
            for notification in notifications {
                send_notification(&finish_events, &state_for_finish, notification).await;
            }
        }

        if let Err(error) = result {
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
                    conversation_id,
                    mode: agent_protocol::FrontendMode::Idle,
                },
            )
            .await;
        }
    })
}

async fn send_notification(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    notification: AppServerNotification,
) {
    let subscribed = {
        let state = state.lock().await;
        state.subscriptions.is_subscribed(notification.conversation_id())
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
    if subscribed {
        let _ = event_tx.send(AppServerMessage::Request(request));
    }
}

