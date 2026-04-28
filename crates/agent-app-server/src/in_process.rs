use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, ApprovalDecision,
    ApprovalRequest, HistoryEntry, RequestId, TurnEvent, TurnResultEnvelope,
};
use agent_runtime::{AgentRuntime, ConversationMessage};
use anyhow::{Result, anyhow};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Debug)]
enum ServerMessage {
    Command(AppClientCommand),
    Shutdown { done: oneshot::Sender<()> },
}

struct ServerState {
    subscribed_sessions: HashSet<String>,
    pending_approvals: HashMap<RequestId, oneshot::Sender<ApprovalDecision>>,
}

impl ServerState {
    fn new(default_session_id: String) -> Self {
        let mut subscribed_sessions = HashSet::new();
        subscribed_sessions.insert(default_session_id);
        Self {
            subscribed_sessions,
            pending_approvals: HashMap::new(),
        }
    }

    fn is_subscribed(&self, session_id: &str) -> bool {
        self.subscribed_sessions.contains(session_id)
    }
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
    runtime: Arc<AgentRuntime>,
    session_id: String,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> InProcessClientHandle {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AppServerMessage>();
    let state = Arc::new(Mutex::new(ServerState::new(session_id.clone())));
    let request_counter = Arc::new(AtomicI64::new(1));

    tokio::spawn(async move {
        while let Some(message) = command_rx.recv().await {
            match message {
                ServerMessage::Command(command) => {
                    if handle_command(
                        runtime.clone(),
                        command,
                        &event_tx,
                        state.clone(),
                        request_counter.clone(),
                        auto_approve,
                        auto_approve_reason.clone(),
                    )
                    .await
                    .is_err()
                    {
                        let _ = event_tx.send(AppServerMessage::Notification(
                            AppServerNotification::Error {
                                session_id: session_id.clone(),
                                message: "command handling failed".to_string(),
                            },
                        ));
                    }
                }
                ServerMessage::Shutdown { done } => {
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

async fn handle_command(
    runtime: Arc<AgentRuntime>,
    command: AppClientCommand,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    request_counter: Arc<AtomicI64>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    match command {
        AppClientCommand::SubmitTurn(input) => {
            send_notification(
                event_tx,
                &state,
                AppServerNotification::FrontendStateChanged {
                    session_id: input.session_id.clone(),
                    mode: agent_protocol::FrontendMode::Running,
                },
            )
            .await;
            spawn_turn(
                runtime,
                input.session_id,
                input.content,
                event_tx.clone(),
                state,
                request_counter,
                auto_approve,
                auto_approve_reason,
            );
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
                state.subscribed_sessions.insert(session_id.clone());
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
                state.subscribed_sessions.remove(&session_id);
            }
            let _ = event_tx.send(AppServerMessage::Notification(
                AppServerNotification::SubscriptionChanged {
                    session_id,
                    subscribed: false,
                },
            ));
        }
        AppClientCommand::ApprovalResponse {
            request_id,
            approved,
            reason,
            ..
        } => {
            let sender = {
                let mut state = state.lock().await;
                state.pending_approvals.remove(&request_id)
            };
            if let Some(reply) = sender {
                let _ = reply.send(ApprovalDecision { approved, reason });
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
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    request_counter: Arc<AtomicI64>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) {
    tokio::spawn(async move {
        let runtime_events = event_tx.clone();
        let finish_events = event_tx.clone();
        let state_for_turn = state.clone();
        let state_for_finish = state.clone();
        let session_id_for_turn = session_id.clone();
        let session_id_for_approval = session_id.clone();

        let result = runtime
            .chat_with_approval_and_events(
                &session_id,
                &user_input,
                move |event| {
                    let runtime_events = runtime_events.clone();
                    let state = state_for_turn.clone();
                    let session_id = session_id_for_turn.clone();
                    let event = event.clone();
                    tokio::spawn(async move {
                        for notification in project_turn_event(&session_id, &event) {
                            send_notification(&runtime_events, &state, notification).await;
                        }
                    });
                },
                move |request: ApprovalRequest| {
                    let event_tx = event_tx.clone();
                    let state = state.clone();
                    let session_id = session_id_for_approval.clone();
                    let request_counter = request_counter.clone();
                    let auto_approve_reason = auto_approve_reason.clone();
                    async move {
                        if auto_approve {
                            return Ok(ApprovalDecision {
                                approved: true,
                                reason: auto_approve_reason
                                    .clone()
                                    .or_else(|| Some("auto-approved by app server".to_string())),
                            });
                        }

                        let request_id =
                            RequestId::Integer(request_counter.fetch_add(1, Ordering::Relaxed));
                        let (reply_tx, reply_rx) = oneshot::channel();
                        {
                            let mut state = state.lock().await;
                            state.pending_approvals.insert(request_id.clone(), reply_tx);
                        }
                        send_request(
                            &event_tx,
                            &state,
                            AppServerRequest::Approval {
                                request_id,
                                session_id,
                                request,
                            },
                        )
                        .await;
                        reply_rx
                            .await
                            .map_err(|_| anyhow!("approval response channel closed"))
                    }
                },
            )
            .await;

        let finish = match result {
            Ok(output) => AppServerNotification::TurnFinished {
                session_id,
                result: TurnResultEnvelope {
                    final_response: output.final_response,
                    state: output.state,
                    error: None,
                },
            },
            Err(error) => AppServerNotification::TurnFinished {
                session_id,
                result: TurnResultEnvelope {
                    final_response: format!("Turn failed: {error:#}"),
                    state: agent_protocol::TurnState::Failed,
                    error: Some(format!("{error:#}")),
                },
            },
        };

        send_notification(&finish_events, &state_for_finish, finish).await;
    });
}

fn project_turn_event(session_id: &str, event: &TurnEvent) -> Vec<AppServerNotification> {
    match event {
        TurnEvent::TurnStarted { turn_id, .. } => vec![AppServerNotification::TurnStarted {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
        }],
        TurnEvent::ItemStarted {
            turn_id,
            item_id,
            kind,
            title,
        } => vec![AppServerNotification::ItemStarted {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
            kind: kind.clone(),
            title: title.clone(),
        }],
        TurnEvent::ItemDelta {
            turn_id,
            item_id,
            kind,
            delta,
        } => match kind {
            agent_protocol::TurnItemDeltaKind::Text => {
                vec![AppServerNotification::AgentMessageDelta {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                }]
            }
            agent_protocol::TurnItemDeltaKind::ToolOutput => {
                vec![AppServerNotification::ToolCallDelta {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                }]
            }
            agent_protocol::TurnItemDeltaKind::ReasoningSummary => {
                vec![AppServerNotification::ReasoningSummaryDelta {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                }]
            }
            agent_protocol::TurnItemDeltaKind::ReasoningText => {
                vec![AppServerNotification::ReasoningTextDelta {
                    session_id: session_id.to_string(),
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                }]
            }
            agent_protocol::TurnItemDeltaKind::JsonPatch => vec![AppServerNotification::PlanDelta {
                session_id: session_id.to_string(),
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                delta: delta.clone(),
            }],
        },
        TurnEvent::ItemCompleted { turn_id, item_id } => vec![AppServerNotification::ItemCompleted {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            item_id: item_id.clone(),
        }],
        TurnEvent::TurnCompleted {
            turn_id,
            final_response,
        } => vec![AppServerNotification::TurnCompleted {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            final_response: final_response.clone(),
        }],
        TurnEvent::TurnFailed { turn_id, error } => vec![AppServerNotification::TurnFailed {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            error: error.clone(),
        }],
        TurnEvent::TurnCancelled { turn_id, reason } => vec![AppServerNotification::TurnCancelled {
            session_id: session_id.to_string(),
            turn_id: turn_id.clone(),
            reason: reason.clone(),
        }],
        TurnEvent::ModelRequestStarted { .. }
        | TurnEvent::ModelResponseReceived { .. }
        | TurnEvent::ApprovalRequested { .. }
        | TurnEvent::ApprovalResolved { .. } => Vec::new(),
    }
}

async fn send_notification(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    notification: AppServerNotification,
) {
    let subscribed = {
        let state = state.lock().await;
        state.is_subscribed(notification.session_id())
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
        state.is_subscribed(request.session_id())
    };
    if subscribed {
        let _ = event_tx.send(AppServerMessage::Request(request));
    }
}

fn history_entry_from_message(message: &ConversationMessage) -> HistoryEntry {
    match message {
        ConversationMessage::System { content } => HistoryEntry::System {
            content: content.clone(),
        },
        ConversationMessage::User { content } => HistoryEntry::User {
            content: content.clone(),
        },
        ConversationMessage::Assistant {
            content,
            tool_calls,
        } => HistoryEntry::Assistant {
            content: content.clone(),
            has_tool_calls: !tool_calls.is_empty(),
        },
        ConversationMessage::Tool {
            tool_call_id,
            name,
            content,
        } => HistoryEntry::Tool {
            tool_call_id: tool_call_id.clone(),
            name: name.clone(),
            content: content.clone(),
        },
    }
}
