use agent_runtime::{AgentRuntime, ApprovalDecision, ConversationMessage, TurnEvent};
use anyhow::{Result, anyhow};
use config::AgentConfig;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
enum CliMessage {
    Event(TurnEvent),
    ApprovalRequest {
        request: agent_runtime::ApprovalRequest,
        reply: oneshot::Sender<ApprovalDecision>,
    },
    TurnFinished(Result<String, String>),
}

struct PendingApproval {
    reply: oneshot::Sender<ApprovalDecision>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = Arc::new(AgentRuntime::from_config(config)?);
    let session_id = runtime.default_session_id().to_string();

    println!("cloudagent session `{session_id}`");
    println!("commands: /exit /reset /history /status /interrupt");

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::stdout();
    let (tx, mut rx) = mpsc::unbounded_channel::<CliMessage>();
    let mut turn_in_progress = false;
    let mut pending_approval: Option<PendingApproval> = None;

    write_prompt(&mut stdout, false).await?;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else {
                    break;
                };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    write_prompt(&mut stdout, pending_approval.is_some()).await?;
                    continue;
                }

                if pending_approval.is_some()
                    && !trimmed.starts_with('/')
                {
                    let approved = matches!(trimmed, "y" | "Y" | "yes" | "YES");
                    let reason = if approved {
                        Some("approved by cli operator".to_string())
                    } else {
                        Some("denied by cli operator".to_string())
                    };
                    if let Some(pending) = pending_approval.take() {
                        let _ = pending.reply.send(ApprovalDecision { approved, reason });
                    }
                    if !turn_in_progress || pending_approval.is_some() {
                        write_prompt(&mut stdout, pending_approval.is_some()).await?;
                    }
                    continue;
                }

                match trimmed {
                    "/exit" | "/quit" => {
                        if turn_in_progress {
                            let interrupted = runtime.interrupt_session(&session_id).await;
                            if interrupted {
                                println!("session> interrupt requested before exit");
                            }
                        }
                        break;
                    }
                    "/reset" => {
                        if turn_in_progress {
                            println!("session> cannot reset while a turn is running");
                            if !turn_in_progress || pending_approval.is_some() {
                                write_prompt(&mut stdout, pending_approval.is_some()).await?;
                            }
                            continue;
                        }
                        runtime.reset_session(&session_id).await?;
                        println!("session reset");
                        write_prompt(&mut stdout, false).await?;
                    }
                    "/history" => {
                        let snapshot = runtime.session_snapshot(&session_id).await?;
                        for message in snapshot.messages {
                            match message {
                                ConversationMessage::System { content } => {
                                    println!("system> {content}");
                                }
                                ConversationMessage::User { content } => {
                                    println!("you> {content}");
                                }
                                ConversationMessage::Assistant { content, .. } => {
                                    if let Some(content) = content {
                                        println!("agent> {content}");
                                    } else {
                                        println!("agent> [tool call]");
                                    }
                                }
                                ConversationMessage::Tool { name, content, .. } => {
                                    println!("tool:{name}> {content}");
                                }
                            }
                        }
                        if !turn_in_progress || pending_approval.is_some() {
                            write_prompt(&mut stdout, pending_approval.is_some()).await?;
                        }
                    }
                    "/status" => {
                        let status = runtime.session_state(&session_id).await?;
                        println!(
                            "session> state={:?} active_turn={:?} turn_state={:?} messages={}",
                            status.session_state, status.active_turn, status.turn_state, status.message_count
                        );
                        if !turn_in_progress || pending_approval.is_some() {
                            write_prompt(&mut stdout, pending_approval.is_some()).await?;
                        }
                    }
                    "/interrupt" => {
                        let interrupted = runtime.interrupt_session(&session_id).await;
                        println!(
                            "session> {}",
                            if interrupted {
                                "interrupt requested"
                            } else {
                                "no active turn"
                            }
                        );
                        if !turn_in_progress || pending_approval.is_some() {
                            write_prompt(&mut stdout, pending_approval.is_some()).await?;
                        }
                    }
                    _ => {
                        if turn_in_progress {
                            println!("session> turn already running; wait, answer approval, or use /interrupt");
                            if !turn_in_progress || pending_approval.is_some() {
                                write_prompt(&mut stdout, pending_approval.is_some()).await?;
                            }
                            continue;
                        }
                        turn_in_progress = true;
                        spawn_turn(runtime.clone(), session_id.clone(), trimmed.to_string(), tx.clone());
                    }
                }
            }
            Some(message) = rx.recv() => {
                match message {
                    CliMessage::Event(event) => {
                        render_event(&event);
                    }
                    CliMessage::ApprovalRequest { request, reply } => {
                        println!(
                            "approval> tool `{}` wants to run with args {}",
                            request.tool_name, request.arguments_preview
                        );
                        println!("approval> {}", request.reason);
                        println!("approval> respond with `y`/`n`, or use /interrupt");
                        pending_approval = Some(PendingApproval { reply });
                        write_prompt(&mut stdout, true).await?;
                    }
                    CliMessage::TurnFinished(result) => {
                        turn_in_progress = false;
                        if pending_approval.is_some() {
                            pending_approval = None;
                        }
                        match result {
                            Ok(final_response) => println!("agent> {final_response}"),
                            Err(error) => println!("turn> failed: {error}"),
                        }
                        write_prompt(&mut stdout, false).await?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn spawn_turn(
    runtime: Arc<AgentRuntime>,
    session_id: String,
    user_input: String,
    tx: mpsc::UnboundedSender<CliMessage>,
) {
    tokio::spawn(async move {
        let event_tx = tx.clone();
        let approval_tx = tx.clone();
        let result = runtime
            .chat_with_approval_and_events(
                &session_id,
                &user_input,
                move |event| {
                    let _ = event_tx.send(CliMessage::Event(event.clone()));
                },
                move |request| {
                    let approval_tx = approval_tx.clone();
                    async move {
                        let (reply_tx, reply_rx) = oneshot::channel();
                        approval_tx
                            .send(CliMessage::ApprovalRequest {
                                request,
                                reply: reply_tx,
                            })
                            .map_err(|_| anyhow!("cli event loop is no longer available"))?;
                        reply_rx
                            .await
                            .map_err(|_| anyhow!("approval response channel closed"))
                    }
                },
            )
            .await
            .map(|output| output.final_response)
            .map_err(|error| format!("{error:#}"));

        let _ = tx.send(CliMessage::TurnFinished(result));
    });
}

async fn write_prompt(stdout: &mut io::Stdout, waiting_for_approval: bool) -> Result<()> {
    let prompt = if waiting_for_approval {
        b"approve> ".as_slice()
    } else {
        b"you> ".as_slice()
    };
    stdout.write_all(prompt).await?;
    stdout.flush().await?;
    Ok(())
}

fn render_event(event: &TurnEvent) {
    match event {
        TurnEvent::TurnStarted { turn_id, .. } => {
            println!("turn> started {turn_id}");
        }
        TurnEvent::ModelRequestStarted {
            message_count,
            tool_count,
            ..
        } => {
            println!(
                "model> requesting completion with {message_count} messages and {tool_count} tools"
            );
        }
        TurnEvent::ModelResponseReceived {
            model_name,
            tool_call_count,
            ..
        } => {
            println!(
                "model> responded from {} with {} tool calls",
                model_name.as_deref().unwrap_or("unknown-model"),
                tool_call_count
            );
        }
        TurnEvent::ToolCallRequested { call, .. } => {
            println!("tool:{}> requested", call.name);
        }
        TurnEvent::ApprovalResolved {
            tool_call_id,
            approved,
            ..
        } => {
            println!(
                "approval> {} for {}",
                if *approved { "approved" } else { "denied" },
                tool_call_id
            );
        }
        TurnEvent::ToolCallCompleted { result, .. } => {
            println!("tool:{}> {}", result.name, result.summary);
        }
        TurnEvent::ToolCallFailed {
            tool_name, error, ..
        } => {
            println!("tool:{}> failed: {}", tool_name, error);
        }
        TurnEvent::TurnFailed { error, .. } => {
            println!("turn> failed: {error}");
        }
        TurnEvent::TurnCancelled { reason, .. } => {
            println!("turn> cancelled: {reason}");
        }
        TurnEvent::AssistantMessage { .. }
        | TurnEvent::ApprovalRequested { .. }
        | TurnEvent::TurnCompleted { .. } => {}
    }
}
