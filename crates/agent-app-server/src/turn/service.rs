use crate::app::notification::{send_notification, send_request};
use crate::routing::command_router::{ServerState, TurnSpawnDependencies};
use crate::server_request::service as server_request_service;
use crate::session::listener::start_conversation_listener;
use crate::session::service as session_service;
use agent_core::{
    AgentHost, ApprovalPolicy, CompactionContinuation, EventMsg, InputItem, PermissionProfile,
    ServerRequest, ServerRequestDecision, TurnState, input_items_text_len,
};
use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn submit_turn(
    runtime: Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
    content: Vec<InputItem>,
    permission_profile: PermissionProfile,
    approval_policy: ApprovalPolicy,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) {
    let created_conversation = runtime
        .ensure_conversation_persisted(&conversation_id)
        .await
        .unwrap_or(false);
    if created_conversation {
        let _ = session_service::list_conversations(&runtime, event_tx, state).await;
    }
    session_service::maybe_spawn_auto_title_job(
        runtime.clone(),
        event_tx.clone(),
        state.clone(),
        conversation_id.clone(),
        content.clone(),
    )
    .await;
    await_tracked_turn_tasks(state, &conversation_id).await;
    send_notification(
        event_tx,
        state,
        AppServerNotification::FrontendStateChanged {
            conversation_id: conversation_id.clone(),
            mode: agent_protocol::FrontendMode::Running,
        },
    )
    .await;
    let task = spawn_turn(
        runtime,
        conversation_id.clone(),
        content,
        permission_profile,
        approval_policy,
        TurnSpawnDependencies {
            event_tx: event_tx.clone(),
            state: state.clone(),
            auto_approve,
            auto_approve_reason,
        },
    );
    state.lock().await.track_turn_task(conversation_id, task);
}

pub(crate) async fn interrupt_turn(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) {
    let interrupted = runtime.interrupt_conversation(&conversation_id).await;
    if interrupted {
        server_request_service::resolve_pending_for_interrupted_conversation(
            event_tx,
            state,
            &conversation_id,
            "interrupted by client",
        )
        .await;
    }
    send_notification(
        event_tx,
        state,
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

pub(crate) async fn compact_conversation(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) -> Result<()> {
    await_tracked_turn_tasks(state, &conversation_id).await;
    let estimated_tokens = runtime
        .conversation_history_snapshot(&conversation_id)
        .await
        .map(|history| estimate_history_tokens(&history.messages))
        .unwrap_or(0) as u64;
    send_notification(
        event_tx,
        state,
        AppServerNotification::ContextCompactionStarted {
            conversation_id: conversation_id.clone(),
            turn_id: "manual_compaction".to_string(),
            continuation: CompactionContinuation::PreTurn,
            estimated_tokens,
        },
    )
    .await;
    match runtime.compact_conversation(&conversation_id).await? {
        agent_core::ManualCompactionOutcome::Compacted {
            pre_context_tokens_estimate,
            post_context_tokens_estimate,
            pre_message_count: _,
            post_message_count: _,
            preserved_tail_count: _,
        } => {
            send_notification(
                event_tx,
                state,
                AppServerNotification::Info {
                    conversation_id,
                    message: format!(
                        "Context compacted: ~{} -> ~{} tokens",
                        pre_context_tokens_estimate, post_context_tokens_estimate
                    ),
                },
            )
            .await;
        }
        agent_core::ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        } => {
            send_notification(
                event_tx,
                state,
                AppServerNotification::Info {
                    conversation_id,
                    message: format!(
                        "Not enough conversation history to compact yet (~{} tokens; need at least ~20000).",
                        estimated_history_tokens
                    ),
                },
            )
            .await;
        }
    }
    Ok(())
}

async fn await_tracked_turn_tasks(state: &Arc<Mutex<ServerState>>, conversation_id: &str) {
    let tasks = {
        state
            .lock()
            .await
            .take_turn_tasks_for_conversation(conversation_id)
    };
    for task in tasks {
        let _ = task.await;
    }
}

fn estimate_history_tokens(messages: &[agent_core::ResponseItem]) -> usize {
    messages
        .iter()
        .map(|item| match item {
            agent_core::ResponseItem::System { content } => content.chars().count(),
            agent_core::ResponseItem::User { content } => input_items_text_len(content),
            agent_core::ResponseItem::Assistant {
                content,
                tool_calls,
            } => {
                let text_len = content.as_ref().map_or(0, |text| text.chars().count());
                let tool_len: usize = tool_calls
                    .iter()
                    .map(|call| {
                        call.name.chars().count() + call.arguments.to_string().chars().count()
                    })
                    .sum();
                text_len + tool_len
            }
            agent_core::ResponseItem::Tool { name, content, .. } => {
                name.chars().count() + content.chars().count()
            }
        })
        .sum::<usize>()
        .saturating_div(3)
        .max(1)
}

fn spawn_turn(
    runtime: Arc<AgentHost>,
    conversation_id: String,
    user_input: Vec<InputItem>,
    permission_profile: PermissionProfile,
    approval_policy: ApprovalPolicy,
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
                &permission_profile,
                &approval_policy,
                move |event| {
                    let event = event.clone();
                    let active_turn_id = active_turn_id_for_events.clone();
                    if let EventMsg::TurnStarted { turn_id, .. } = &event
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
                            return Ok(ServerRequestDecision::accept(
                                auto_approve_reason
                                    .clone()
                                    .or_else(|| Some("auto-approved by app server".to_string())),
                            ));
                        }
                        let request_id = { state.lock().await.next_server_request_id() };
                        let (reply_tx, reply_rx) = oneshot::channel();
                        let turn_id = match &request {
                            ServerRequest::CommandApproval { request } => request.turn_id.clone(),
                            ServerRequest::FileChangeApproval { request } => {
                                request.turn_id.clone()
                            }
                        };
                        {
                            state.lock().await.insert_pending_server_request(
                                request_id.clone(),
                                conversation_id.clone(),
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
        {
            state_for_finish
                .lock()
                .await
                .clear_active_listener(&conversation_id);
        }
        match result {
            Ok(output) => {
                server_request_service::resolve_pending_for_finished_turn(
                    &finish_events,
                    &state_for_finish,
                    &conversation_id,
                    &output.turn_id,
                    "turn finished before request was answered",
                )
                .await;
                listener.finish_turn(output.state).await;
            }
            Err(error) => {
                let maybe_turn_id = active_turn_id.lock().ok().and_then(|guard| guard.clone());
                let error_text = format!("{error:#}");
                let busy_error = error_text.starts_with("ERR_CONVERSATION_BUSY:");
                if let Some(turn_id) = maybe_turn_id {
                    server_request_service::resolve_pending_for_finished_turn(
                        &finish_events,
                        &state_for_finish,
                        &conversation_id,
                        &turn_id,
                        "turn failed before request was answered",
                    )
                    .await;
                    send_notification(
                        &finish_events,
                        &state_for_finish,
                        AppServerNotification::TurnFailed {
                            conversation_id: conversation_id.clone(),
                            turn_id,
                            error: error_text,
                        },
                    )
                    .await;
                } else {
                    let message = if busy_error {
                        "conversation is busy; wait for the active turn to finish or interrupt it"
                            .to_string()
                    } else {
                        format!("turn failed before start: {error:#}")
                    };
                    send_notification(
                        &finish_events,
                        &state_for_finish,
                        AppServerNotification::Error {
                            conversation_id: conversation_id.clone(),
                            message,
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
                listener.finish_turn(TurnState::Failed).await;
            }
        }
        let _ = listener_task.await;
    })
}
