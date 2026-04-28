use crate::ConsoleConfig;
use crate::event::ControllerEvent;
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

    let (tx, mut rx) = mpsc::unbounded_channel::<ControllerEvent>();
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<ParsedInput>();
    let (mode_tx, mode_rx) = watch::channel(FrontendMode::Idle);
    let mut state = ConsoleState::new();
    let mut renderer = ConsoleRenderer::new(config.banner);

    spawn_input_loop(session_id.clone(), input_tx, mode_rx);
    renderer.render_banner().await?;

    loop {
        tokio::select! {
            biased;
            Some(message) = rx.recv() => {
                handle_controller_event(&mut state, &mut renderer, message).await?;
                let _ = mode_tx.send(state.mode());
            }
            Some(input) = input_rx.recv() => {
                match input {
                    ParsedInput::Empty => renderer.render_prompt(state.mode()).await?,
                    ParsedInput::Command(command) => {
                        if handle_command(
                            &runtime,
                            &session_id,
                            &mut state,
                            &mut renderer,
                            command,
                            tx.clone(),
                            auto_approve,
                            auto_approve_reason.clone(),
                        ).await? {
                            break;
                        }
                        let _ = mode_tx.send(state.mode());
                    }
                }
            }
            else => break,
        }
    }

    Ok(())
}

fn spawn_input_loop(
    session_id: String,
    input_tx: mpsc::UnboundedSender<ParsedInput>,
    mode_rx: watch::Receiver<FrontendMode>,
) {
    tokio::spawn(async move {
        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();

        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) | Err(_) => {
                    let _ = input_tx.send(ParsedInput::Command(AppClientCommand::Exit));
                    break;
                }
            };

            let mode = *mode_rx.borrow();
            if input_tx.send(parse_line(&line, &session_id, mode)).is_err() {
                break;
            }
        }
    });
}

async fn handle_command(
    runtime: &Arc<AgentRuntime>,
    session_id: &str,
    state: &mut ConsoleState,
    renderer: &mut ConsoleRenderer,
    command: AppClientCommand,
    tx: mpsc::UnboundedSender<ControllerEvent>,
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
                    tx,
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

async fn handle_controller_event(
    state: &mut ConsoleState,
    renderer: &mut ConsoleRenderer,
    message: ControllerEvent,
) -> Result<()> {
    match message {
        ControllerEvent::Protocol(event) => {
            state.update_from_protocol(&event);
            renderer.render_protocol_event(&event).await?;
            renderer.render_prompt(state.mode()).await?;
        }
        ControllerEvent::Runtime(event) => {
            renderer
                .render_protocol_event(&AppServerEvent::TurnEvent {
                    session_id: String::new(),
                    event,
                })
                .await?;
        }
        ControllerEvent::ApprovalRequest { request, reply } => {
            state.set_pending_approval(request.clone(), reply);
            let mode_event = AppServerEvent::FrontendStateChanged {
                session_id: String::new(),
                mode: FrontendMode::WaitingForApproval,
            };
            state.update_from_protocol(&mode_event);
            renderer
                .render_protocol_event(&AppServerEvent::ApprovalPrompt {
                    session_id: String::new(),
                    request,
                })
                .await?;
            renderer.render_prompt(state.mode()).await?;
        }
    }
    Ok(())
}

fn spawn_turn(
    runtime: Arc<AgentRuntime>,
    session_id: String,
    user_input: String,
    tx: mpsc::UnboundedSender<ControllerEvent>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) {
    tokio::spawn(async move {
        let event_tx = tx.clone();
        let approval_tx = tx.clone();
        let result = runtime
            .chat_with_approval_and_events(
                &session_id,
                &user_input,
                move |event| {
                    let _ = event_tx.send(ControllerEvent::Runtime(event.clone()));
                },
                move |request| {
                    let approval_tx = approval_tx.clone();
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
                        approval_tx
                            .send(ControllerEvent::ApprovalRequest {
                                request,
                                reply: reply_tx,
                            })
                            .map_err(|_| anyhow!("console controller is no longer available"))?;
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
            Ok(result) => AppServerEvent::TurnFinished {
                session_id,
                result,
            },
            Err(error) => AppServerEvent::TurnFinished {
                session_id,
                result: TurnResultEnvelope {
                    final_response: format!("Turn failed: {error}"),
                    state: agent_protocol::TurnState::Failed,
                    error: Some(error),
                },
            },
        };

        let _ = tx.send(ControllerEvent::Protocol(finish_event));
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
