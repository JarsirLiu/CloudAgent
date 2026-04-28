use agent_runtime::AgentRuntime;
use anyhow::Result;
use config::AgentConfig;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use agent_runtime::TurnEvent;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = AgentRuntime::from_config(config)?;
    let session_id = runtime.default_session_id().to_string();

    println!("cloudagent session `{session_id}`");
    println!("commands: /exit /reset /history /status /interrupt");

    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::stdout();

    loop {
        stdout.write_all(b"you> ").await?;
        stdout.flush().await?;

        let Some(line) = lines.next_line().await? else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match trimmed {
            "/exit" | "/quit" => break,
            "/reset" => {
                runtime.reset_session(&session_id).await?;
                println!("session reset");
            }
            "/history" => {
                let snapshot = runtime.session_snapshot(&session_id).await?;
                for message in snapshot.messages {
                    match message {
                        agent_runtime::ConversationMessage::System { content } => {
                            println!("system> {content}");
                        }
                        agent_runtime::ConversationMessage::User { content } => {
                            println!("you> {content}");
                        }
                        agent_runtime::ConversationMessage::Assistant { content, .. } => {
                            if let Some(content) = content {
                                println!("agent> {content}");
                            } else {
                                println!("agent> [tool call]");
                            }
                        }
                        agent_runtime::ConversationMessage::Tool { name, content, .. } => {
                            println!("tool:{name}> {content}");
                        }
                    }
                }
            }
            "/status" => {
                let status = runtime.session_state(&session_id).await?;
                println!(
                    "session> state={:?} active_turn={:?} turn_state={:?} messages={}",
                    status.session_state, status.active_turn, status.turn_state, status.message_count
                );
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
            }
            _ => {
                let output = runtime
                    .chat_with_approval_and_events(
                        &session_id,
                        trimmed,
                        |event| render_event(event),
                        |request| async move {
                            println!(
                                "approval> tool `{}` wants to run with args {}",
                                request.tool_name, request.arguments_preview
                            );
                            println!("approval> {}", request.reason);
                            let mut prompt_out = io::stdout();
                            prompt_out.write_all(b"approve? [y/N]: ").await?;
                            prompt_out.flush().await?;
                            let mut answer = String::new();
                            let mut input = BufReader::new(io::stdin());
                            input.read_line(&mut answer).await?;
                            let approved = matches!(answer.trim(), "y" | "Y" | "yes" | "YES");
                            Ok(agent_runtime::ApprovalDecision {
                                approved,
                                reason: if approved {
                                    Some("approved by cli operator".to_string())
                                } else {
                                    Some("denied by cli operator".to_string())
                                },
                            })
                        },
                    )
                    .await?;
                println!("agent> {}", output.final_response);
            }
        }
    }

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
