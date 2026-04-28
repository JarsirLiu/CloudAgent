use agent_runtime::{AgentRuntime, TurnEvent};
use anyhow::Result;
use config::AgentConfig;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

#[derive(Debug)]
enum ConsoleMessage {
    Event(TurnEvent),
    TurnFinished(Result<String, String>),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = Arc::new(AgentRuntime::from_config(config)?);

    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "console") {
        run_console(runtime).await?;
        return Ok(());
    }

    tracing::info!(
        "agentd ready; session store at {}",
        runtime.default_session_id()
    );
    tracing::info!("run `cargo run -p agentd -- console` to attach a local console");
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn run_console(runtime: Arc<AgentRuntime>) -> Result<()> {
    let session_id = runtime.default_session_id().to_string();
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::stdout();
    let (tx, mut rx) = mpsc::unbounded_channel::<ConsoleMessage>();
    let mut turn_in_progress = false;

    println!("agentd console attached to session `{session_id}`");
    stdout.write_all(b"daemon-you> ").await?;
    stdout.flush().await?;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else {
                    break;
                };
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    if !turn_in_progress {
                        stdout.write_all(b"daemon-you> ").await?;
                        stdout.flush().await?;
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
                        if !turn_in_progress {
                            stdout.write_all(b"daemon-you> ").await?;
                            stdout.flush().await?;
                        }
                    }
                    _ => {
                        if turn_in_progress {
                            println!("session> turn already running; wait or use /interrupt");
                            if !turn_in_progress {
                                stdout.write_all(b"daemon-you> ").await?;
                                stdout.flush().await?;
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
                    ConsoleMessage::Event(event) => render_event(&event),
                    ConsoleMessage::TurnFinished(result) => {
                        turn_in_progress = false;
                        match result {
                            Ok(final_response) => println!("agent> {final_response}"),
                            Err(error) => println!("turn> failed: {error}"),
                        }
                        stdout.write_all(b"daemon-you> ").await?;
                        stdout.flush().await?;
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
    tx: mpsc::UnboundedSender<ConsoleMessage>,
) {
    tokio::spawn(async move {
        let event_tx = tx.clone();
        let result = runtime
            .chat_with_approval_and_events(
                &session_id,
                &user_input,
                move |event| {
                    let _ = event_tx.send(ConsoleMessage::Event(event.clone()));
                },
                |_request| async move {
                    Ok(agent_runtime::ApprovalDecision {
                        approved: true,
                        reason: Some("auto-approved in local daemon console".to_string()),
                    })
                },
            )
            .await
            .map(|output| output.final_response)
            .map_err(|error| format!("{error:#}"));

        let _ = tx.send(ConsoleMessage::TurnFinished(result));
    });
}

fn render_event(event: &TurnEvent) {
    match event {
        TurnEvent::TurnStarted { turn_id, .. } => println!("turn> started {turn_id}"),
        TurnEvent::ModelRequestStarted { .. } => println!("model> request started"),
        TurnEvent::ModelResponseReceived {
            model_name,
            tool_call_count,
            ..
        } => println!(
            "model> {} returned {} tool calls",
            model_name.as_deref().unwrap_or("unknown-model"),
            tool_call_count
        ),
        TurnEvent::ToolCallRequested { call, .. } => {
            println!("tool:{}> requested", call.name)
        }
        TurnEvent::ApprovalResolved { approved, .. } => println!(
            "approval> {}",
            if *approved { "approved" } else { "denied" }
        ),
        TurnEvent::ToolCallCompleted { result, .. } => {
            println!("tool:{}> {}", result.name, result.summary)
        }
        TurnEvent::ToolCallFailed {
            tool_name, error, ..
        } => println!("tool:{}> failed: {}", tool_name, error),
        TurnEvent::TurnFailed { error, .. } => println!("turn> failed: {error}"),
        TurnEvent::TurnCancelled { reason, .. } => println!("turn> cancelled: {reason}"),
        TurnEvent::AssistantMessage { .. }
        | TurnEvent::ApprovalRequested { .. }
        | TurnEvent::TurnCompleted { .. } => {}
    }
}
