use crate::app::notification::{send_notification, send_request};
use crate::routing::command_router::{AppSessionServices, ServerState, TurnSpawnDependencies};
use crate::server_request::service as server_request_service;
use crate::server_request::view::pending_request_view;
use crate::session::listener::start_conversation_listener;
use crate::session::service as session_service;
use agent_core::{
    AgentHost, ApprovalPolicy, CompactionContinuation, EventMsg, InputItem, PermissionProfile,
    ServerRequest, ServerRequestDecision, TurnState, input_items_text_len,
};
use agent_protocol::{
    AppServerNotification, AppServerRequest, InterruptDisposition, TurnViewStatus,
};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn submit_turn(
    runtime: Arc<AgentHost>,
    services: &AppSessionServices,
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
        let _ = session_service::list_conversations_page(
            &runtime,
            &services.event_tx,
            &services.state,
            None,
            25,
        )
        .await;
    }
    session_service::maybe_spawn_auto_title_job(
        runtime.clone(),
        services.event_tx.clone(),
        services.state.clone(),
        conversation_id.clone(),
        content.clone(),
    )
    .await;
    await_tracked_turn_tasks(&services.state, &conversation_id).await;
    let task = spawn_turn(
        runtime,
        conversation_id.clone(),
        content,
        permission_profile,
        approval_policy,
        TurnSpawnDependencies {
            services: services.clone(),
            auto_approve,
            auto_approve_reason,
        },
    );
    services
        .state
        .lock()
        .await
        .track_turn_task(conversation_id, task);
}

pub(crate) async fn interrupt_turn(
    runtime: &Arc<AgentHost>,
    services: &AppSessionServices,
    conversation_id: String,
) {
    let event_tx = &services.event_tx;
    let state = &services.state;
    let view = &services.view;
    let interrupted = runtime.interrupt_conversation(&conversation_id).await;
    if interrupted {
        view.note_interrupt_requested(&conversation_id).await;
        server_request_service::resolve_pending_for_interrupted_conversation(
            event_tx,
            state,
            view,
            &conversation_id,
            "interrupted by client",
        )
        .await;
    } else {
        view.emit_current(&conversation_id).await;
    }
    send_notification(
        event_tx,
        state,
        AppServerNotification::InterruptResult {
            conversation_id,
            disposition: if interrupted {
                InterruptDisposition::Requested
            } else {
                InterruptDisposition::NoActiveTurn
            },
        },
    )
    .await;
}

pub(crate) async fn compact_conversation(
    runtime: &Arc<AgentHost>,
    services: &AppSessionServices,
    conversation_id: String,
) -> Result<()> {
    let event_tx = &services.event_tx;
    let state = &services.state;
    let view = &services.view;
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
            turn_id: None,
            continuation: CompactionContinuation::PreTurn,
            estimated_tokens,
        },
    )
    .await;
    view.note_compaction_started(&conversation_id).await;
    match runtime.compact_conversation(&conversation_id).await? {
        agent_core::ManualCompactionOutcome::Compacted {
            pre_context_tokens_estimate,
            post_context_tokens_estimate,
            pre_message_count: _,
            post_message_count: _,
            preserved_user_count: _,
        } => {
            send_notification(
                event_tx,
                state,
                AppServerNotification::Info {
                    conversation_id: conversation_id.clone(),
                    message: format!(
                        "Context compacted: ~{} -> ~{} tokens",
                        pre_context_tokens_estimate, post_context_tokens_estimate
                    ),
                },
            )
            .await;
            view.note_compaction_finished(&conversation_id).await;
        }
        agent_core::ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        } => {
            send_notification(
                event_tx,
                state,
                AppServerNotification::Info {
                    conversation_id: conversation_id.clone(),
                    message: format!(
                        "Not enough conversation history to compact yet (~{} tokens; need at least ~20000).",
                        estimated_history_tokens
                    ),
                },
            )
            .await;
            view.note_compaction_finished(&conversation_id).await;
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
                ..
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
        let finish_events = deps.services.event_tx.clone();
        let state_for_finish = deps.services.state.clone();
        let view = deps.services.view.clone();
        let conversation_id_for_server_request = conversation_id.clone();
        let active_turn_id = Arc::new(StdMutex::new(None::<String>));
        let active_turn_id_for_events = active_turn_id.clone();
        let runtime_for_requests = runtime.clone();
        let view_for_requests = view.clone();
        let view_for_events = view.clone();
        let conversation_id_for_events = conversation_id.clone();
        let (listener, listener_task) = start_conversation_listener(
            conversation_id.clone(),
            deps.services.event_tx.clone(),
            deps.services.state.clone(),
        );
        {
            let mut state = deps.services.state.lock().await;
            state.set_active_listener(conversation_id.clone(), listener.clone());
        }
        view.note_turn_starting(&conversation_id).await;
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
                    let view = view_for_events.clone();
                    let conversation_id = conversation_id_for_events.clone();
                    let listener = listener_for_events.clone();
                    if let EventMsg::TurnStarted { turn_id, .. } = &event
                        && let Ok(mut active) = active_turn_id.lock()
                    {
                        *active = Some(turn_id.clone());
                    }
                    let event_for_view = event.clone();
                    listener.project_event(event);
                    tokio::spawn(async move {
                        match &event_for_view {
                            EventMsg::TurnStarted { turn_id, .. } => {
                                view.note_turn_started(&conversation_id, turn_id.clone())
                                    .await;
                            }
                            EventMsg::ContextCompactionStarted { .. } => {
                                view.note_compaction_started(&conversation_id).await;
                            }
                            EventMsg::ContextCompacted { .. } => {
                                view.note_compaction_finished(&conversation_id).await;
                            }
                            _ => {}
                        }
                        let active_turn = listener.active_turn_snapshot().await;
                        view.note_active_turn_snapshot(&conversation_id, active_turn)
                            .await;
                    });
                },
                move |request: ServerRequest| {
                    let view = view_for_requests.clone();
                    let state = deps.services.state.clone();
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
                        let request_guard = view
                            .note_server_request_pending(
                                &conversation_id,
                                pending_request_view(
                                    &conversation_id,
                                    request_id.clone(),
                                    &request,
                                    0,
                                ),
                            )
                            .await;
                        send_request(
                            view.event_tx(),
                            view.server_state(),
                            AppServerRequest::ServerRequest {
                                request_id,
                                conversation_id,
                                request,
                            },
                        )
                        .await;
                        let decision = reply_rx
                            .await
                            .map_err(|_| anyhow!("server request response channel closed"));
                        drop(request_guard);
                        decision
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
                    &view,
                    &conversation_id,
                    &output.turn_id,
                    "turn finished before request was answered",
                )
                .await;
                let turn_view_status = turn_view_status_from_core(output.state.clone());
                listener.finish_turn(output.state).await;
                view.note_turn_finished(&conversation_id, turn_view_status)
                    .await;
            }
            Err(error) => {
                let maybe_turn_id = active_turn_id.lock().ok().and_then(|guard| guard.clone());
                let error_text = format!("{error:#}");
                let busy_error = error_text.starts_with("ERR_CONVERSATION_BUSY:");
                if let Some(turn_id) = maybe_turn_id {
                    server_request_service::resolve_pending_for_finished_turn(
                        &finish_events,
                        &state_for_finish,
                        &view,
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
                    view.note_system_error(&conversation_id, message.clone())
                        .await;
                    listener.system_error(message);
                }
                listener.finish_turn(TurnState::Failed).await;
                view.note_turn_finished(&conversation_id, TurnViewStatus::Failed)
                    .await;
            }
        }
        let _ = listener_task.await;
    })
}

fn turn_view_status_from_core(turn_state: TurnState) -> TurnViewStatus {
    match turn_state {
        TurnState::Completed => TurnViewStatus::Completed,
        TurnState::Cancelled => TurnViewStatus::Interrupted,
        TurnState::Failed => TurnViewStatus::Failed,
        TurnState::Idle | TurnState::Running | TurnState::WaitingForServerRequest => {
            TurnViewStatus::Failed
        }
    }
}
