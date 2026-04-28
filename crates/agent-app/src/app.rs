use crate::ConsoleConfig;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::input::{ParsedInput, parse_line};
use crate::render::ConsoleRenderer;
use crate::state::ConsoleState;
use agent_protocol::{
    AppClientCommand, AppServerEvent, FrontendMode, HistoryEntry, TurnResultEnvelope,
};
use agent_runtime::{AgentRuntime, ApprovalDecision, ConversationMessage};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

pub(crate) async fn run(runtime: Arc<AgentRuntime>, config: ConsoleConfig) -> Result<()> {
    let session_id = config.session_id.clone();
    let auto_approve = config.auto_approve;
    let auto_approve_reason = config.auto_approve_reason.clone();

    let (app_event_tx, mut app_event_rx) = mpsc::unbounded_channel::<AppEvent>();
    let app_event_tx = AppEventSender::new(app_event_tx);
    let (mode_tx, mode_rx) = watch::channel(FrontendMode::Idle);
    let mut state = ConsoleState::new();
    let mut renderer = ConsoleRenderer::new(config.banner);

    spawn_input_loop(session_id.clone(), app_event_tx.clone(), mode_rx);
    renderer.render_banner().await?;

    loop {
        let Some(event) = app_event_rx.recv().await else {
            break;
        };

        if handle_app_event(
            &runtime,
            &session_id,
            &mut state,
            &mut renderer,
            event,
            app_event_tx.clone(),
            auto_approve,
            auto_approve_reason.clone(),
        )
        .await?
        {
            break;
        }

        let _ = mode_tx.send(state.mode());
    }

    Ok(())
}

fn spawn_input_loop(
    session_id: String,
    app_event_tx: AppEventSender,
    mode_rx: watch::Receiver<FrontendMode>,
) {
    tokio::spawn(async move {
        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();

        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) | Err(_) => {
                    app_event_tx.input(ParsedInput::Command(AppClientCommand::Exit));
                    break;
                }
            };

            let mode = *mode_rx.borrow();
            app_event_tx.input(parse_line(&line, &session_id, mode));
        }
    });
}

async fn handle_app_event(
    runtime: &Arc<AgentRuntime>,
    session_id: &str,
    state: &mut ConsoleState,
    renderer: &mut ConsoleRenderer,
    event: AppEvent,
    app_event_tx: AppEventSender,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<bool> {
    match event {
        AppEvent::Input(ParsedInput::Empty) => {
            renderer.render_prompt(state.mode()).await?;
        }
        AppEvent::Input(ParsedInput::Command(command)) => {
            if handle_command(
                runtime,
                session_id,
                state,
                renderer,
                command,
                app_event_tx,
                auto_approve,
                auto_approve_reason,
            )
            .await?
            {
                return Ok(true);
            }
        }
        AppEvent::ProtocolEvent(event) => {
            state.update_from_protocol(&event);
            renderer.render_protocol_event(&event).await?;
            renderer.render_prompt(state.mode()).await?;
        }
        AppEvent::RuntimeEvent(event) => {
            renderer
                .render_protocol_event(&AppServerEvent::TurnEvent {
                    session_id: String::new(),
                    event,
                })
                .await?;
        }
        AppEvent::ApprovalRequest { request, reply } => {
            state.set_pending_approval(request.clone(), reply);
            state.update_from_protocol(&AppServerEvent::FrontendStateChanged {
                session_id: String::new(),
                mode: FrontendMode::WaitingForApproval,
            });
            renderer
                .render_protocol_event(&AppServerEvent::ApprovalPrompt {
                    session_id: String::new(),
                    request,
                })
                .await?;
            renderer.render_prompt(state.mode()).await?;
        }
    }

    Ok(false)
}

async fn handle_command(
    runtime: &Arc<AgentRuntime>,
    session_id: &str,
    state: &mut ConsoleState,
    renderer: &mut ConsoleRenderer,
    command: AppClientCommand,
    app_event_tx: AppEventSender,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<bool> {
    let should_render_prompt = !matches!(
        command,
        AppClientCommand::SubmitTurn(_) | AppClientCommand::ApprovalResponse { .. }
    );

    match command {
        AppClientCommand::Exit => {
            if state.mode() != FrontendMode::Idle {
                let interrupted = runtime.interrupt_session(session_id).await;
                if interrupted {
                    renderer
                        .render_protocol_event(&AppServerEvent::Info {
                            session_id: session_id.to_string(),
                            message: "interrupt requested before exit".to_string(),
                        })
                        .await?;
                }
            }
            return Ok(true);
        }
        AppClientCommand::SubmitTurn(input) => {
            if !state.can_submit_turn() {
                renderer
                    .render_protocol_event(&AppServerEvent::Info {
                        session_id: session_id.to_string(),
                        message: "turn already running; wait, answer approval, or use /interrupt"
                            .to_string(),
                    })
                    .await?;
            } else {
                state.update_from_protocol(&AppServerEvent::FrontendStateChanged {
                    session_id: session_id.to_string(),
                    mode: FrontendMode::Running,
                });
                spawn_turn(
                    runtime.clone(),
                    input.session_id,
                    input.content,
                    app_event_tx,
                    auto_approve,
                    auto_approve_reason,
                );
            }
        }
        AppClientCommand::ApprovalResponse { approved, reason, .. } => {
            if let Some(pending) = state.take_pending_approval() {
                let _ = pending.reply.send(ApprovalDecision { approved, reason });
                state.update_from_protocol(&AppServerEvent::FrontendStateChanged {
                    session_id: session_id.to_string(),
                    mode: FrontendMode::Running,
                });
            } else {
                renderer
                    .render_protocol_event(&AppServerEvent::Info {
                        session_id: session_id.to_string(),
                        message: "no pending approval".to_string(),
                    })
                    .await?;
            }
        }
        AppClientCommand::InterruptTurn { session_id } => {
            let interrupted = runtime.interrupt_session(&session_id).await;
            renderer
                .render_protocol_event(&AppServerEvent::Info {
                    session_id,
                    message: if interrupted {
                        "interrupt requested".to_string()
                    } else {
                        "no active turn".to_string()
                    },
                })
                .await?;
        }
        AppClientCommand::ResetSession { session_id } => {
            if !state.can_submit_turn() {
                renderer
                    .render_protocol_event(&AppServerEvent::Info {
                        session_id,
                        message: "cannot reset while a turn is running".to_string(),
                    })
                    .await?;
            } else {
                runtime.reset_session(&session_id).await?;
                renderer
                    .render_protocol_event(&AppServerEvent::Info {
                        session_id,
                        message: "session reset".to_string(),
                    })
                    .await?;
            }
        }
        AppClientCommand::RequestStatus { session_id } => {
            let snapshot = runtime.session_state(&session_id).await?;
            renderer
                .render_protocol_event(&AppServerEvent::SessionStatus {
                    session_id,
                    snapshot,
                })
                .await?;
        }
        AppClientCommand::RequestHistory { session_id } => {
            let snapshot = runtime.session_snapshot(&session_id).await?;
            renderer
                .render_protocol_event(&AppServerEvent::SessionHistory {
                    session_id,
                    messages: snapshot.messages.iter().map(history_entry_from_message).collect(),
                })
                .await?;
        }
    }

    if should_render_prompt {
        renderer.render_prompt(state.mode()).await?;
    }
    Ok(false)
}

fn spawn_turn(
    runtime: Arc<AgentRuntime>,
    session_id: String,
    user_input: String,
    app_event_tx: AppEventSender,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) {
    tokio::spawn(async move {
        let runtime_events = app_event_tx.clone();
        let approval_events = app_event_tx.clone();
        let result = runtime
            .chat_with_approval_and_events(
                &session_id,
                &user_input,
                move |event| runtime_events.runtime_event(event.clone()),
                move |request| {
                    let approval_events = approval_events.clone();
                    let auto_approve_reason = auto_approve_reason.clone();
                    async move {
                        if auto_approve {
                            return Ok(ApprovalDecision {
                                approved: true,
                                reason: auto_approve_reason
                                    .clone()
                                    .or_else(|| Some("auto-approved by console".to_string())),
                            });
                        }

                        let (reply_tx, reply_rx) = oneshot::channel();
                        approval_events.approval_request(request, reply_tx);
                        reply_rx
                            .await
                            .map_err(|_| anyhow!("approval response channel closed"))
                    }
                },
            )
            .await
            .map(|output| TurnResultEnvelope {
                final_response: output.final_response,
                state: output.state,
                error: None,
            })
            .map_err(|error| format!("{error:#}"));

        let finish_event = match result {
            Ok(result) => AppServerEvent::TurnFinished { session_id, result },
            Err(error) => AppServerEvent::TurnFinished {
                session_id,
                result: TurnResultEnvelope {
                    final_response: format!("Turn failed: {error}"),
                    state: agent_protocol::TurnState::Failed,
                    error: Some(error),
                },
            },
        };
        app_event_tx.protocol_event(finish_event);
    });
}

fn history_entry_from_message(message: &ConversationMessage) -> HistoryEntry {
    match message {
        ConversationMessage::System { content } => HistoryEntry::System {
            content: content.clone(),
        },
        ConversationMessage::User { content } => HistoryEntry::User {
            content: content.clone(),
        },
        ConversationMessage::Assistant { content, tool_calls } => HistoryEntry::Assistant {
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
