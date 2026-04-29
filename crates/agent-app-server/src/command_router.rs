use crate::server_request_coordinator::ServerRequestCoordinator;
use crate::session_subscriptions::SessionSubscriptions;
use crate::turn_bridge::{history_entry_from_message, project_turn_event};
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
    subscriptions: SessionSubscriptions,
    server_requests: ServerRequestCoordinator,
    turn_tasks: Vec<JoinHandle<()>>,
}

impl ServerState {
    pub(crate) fn new(default_session_id: String) -> Self {
        Self {
            subscriptions: SessionSubscriptions::new(default_session_id),
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
                    session_id: input.session_id.clone(),
                    mode: agent_protocol::FrontendMode::Running,
                },
            )
            .await;
            let task = spawn_turn(
                runtime,
                input.session_id,
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
        AppClientCommand::InterruptTurn { session_id } => {
            let interrupted = runtime.interrupt_session(&session_id).await;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::Info {
                    session_id,
                    message: if interrupted {
                        "interrupt requested".to_string()
                    } else {
                        "no active turn".to_string()
                    },
                },
            )
            .await;
        }
        AppClientCommand::RequestStatus { session_id } => {
            let snapshot = runtime.session_state(&session_id).await?;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::SessionStatus {
                    session_id,
                    snapshot,
                },
            )
            .await;
        }
        AppClientCommand::RequestHistory { session_id } => {
            let snapshot = runtime.session_snapshot(&session_id).await?;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::SessionHistory {
                    session_id,
                    messages: snapshot
                        .messages
                        .iter()
                        .map(history_entry_from_message)
                        .collect(),
                },
            )
            .await;
        }
        AppClientCommand::RequestEventLog { session_id } => {
            let events = runtime.session_events(&session_id).await?;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::SessionEventLog { session_id, events },
            )
            .await;
        }
        AppClientCommand::ResetSession { session_id } => {
            runtime.reset_session(&session_id).await?;
            send_notification(
                event_tx,
                &state,
                AppServerNotification::Info {
                    session_id,
                    message: "session reset".to_string(),
                },
            )
            .await;
        }
        AppClientCommand::SubscribeSession { session_id } => {
            {
                let mut state = state.lock().await;
                state.subscriptions.subscribe(session_id.clone());
            }
            send_notification(
                event_tx,
                &state,
                AppServerNotification::SubscriptionChanged {
                    session_id,
                    subscribed: true,
                },
            )
            .await;
        }
        AppClientCommand::UnsubscribeSession { session_id } => {
            {
                let mut state = state.lock().await;
                state.subscriptions.unsubscribe(&session_id);
            }
            let _ = event_tx.send(AppServerMessage::Notification(
                AppServerNotification::SubscriptionChanged {
                    session_id,
                    subscribed: false,
                },
            ));
        }
        AppClientCommand::ResolveServerRequest {
            session_id,
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
                send_notification(
                    event_tx,
                    &state,
                    AppServerNotification::ServerRequestResolved {
                        session_id,
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
    session_id: String,
    user_input: String,
    ctx: SpawnTurnContext,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let runtime_events = ctx.event_tx.clone();
        let finish_events = ctx.event_tx.clone();
        let state_for_turn = ctx.state.clone();
        let state_for_finish = ctx.state.clone();
        let session_id_for_turn = session_id.clone();
        let session_id_for_server_request = session_id.clone();
        let active_turn_id = Arc::new(StdMutex::new(None::<String>));
        let active_turn_id_for_events = active_turn_id.clone();
        let (projected_tx, mut projected_rx) =
            mpsc::unbounded_channel::<Vec<AppServerNotification>>();
        let runtime_events_for_projected = runtime_events.clone();
        let state_for_projected = state_for_turn.clone();
        tokio::spawn(async move {
            while let Some(notifications) = projected_rx.recv().await {
                for notification in notifications {
                    send_notification(&runtime_events_for_projected, &state_for_projected, notification)
                        .await;
                }
            }
        });

        let result = runtime
            .chat_with_approval_and_events(
                &session_id,
                &user_input,
                move |event| {
                    let session_id = session_id_for_turn.clone();
                    let event = event.clone();
                    let active_turn_id = active_turn_id_for_events.clone();
                    let projected_tx = projected_tx.clone();
                    if let agent_protocol::TurnEvent::TurnStarted { turn_id, .. } = &event
                        && let Ok(mut active) = active_turn_id.lock()
                    {
                        *active = Some(turn_id.clone());
                    }
                    let notifications = project_turn_event(&session_id, &event);
                    let _ = projected_tx.send(notifications);
                },
                move |request: ServerRequest| {
                    let event_tx = ctx.event_tx.clone();
                    let state = ctx.state.clone();
                    let session_id = session_id_for_server_request.clone();
                    let auto_approve_reason = ctx.auto_approve_reason.clone();
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
                        send_request(
                            &event_tx,
                            &state,
                        AppServerRequest::ServerRequest {
                            request_id,
                            session_id,
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

        if let Err(error) = result {
            let maybe_turn_id = active_turn_id.lock().ok().and_then(|guard| guard.clone());
            if let Some(turn_id) = maybe_turn_id {
                send_notification(
                    &finish_events,
                    &state_for_finish,
                    AppServerNotification::TurnFailed {
                        session_id: session_id.clone(),
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
                        session_id: session_id.clone(),
                        message: format!("turn failed before start: {error:#}"),
                    },
                )
                .await;
            }
            send_notification(
                &finish_events,
                &state_for_finish,
                AppServerNotification::FrontendStateChanged {
                    session_id,
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
        state.subscriptions.is_subscribed(notification.session_id())
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
        state.subscriptions.is_subscribed(request.session_id())
    };
    if subscribed {
        let _ = event_tx.send(AppServerMessage::Request(request));
    }
}
