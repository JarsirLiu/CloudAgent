use agent_runtime::AgentRuntime;
use anyhow::Result;
use config::AgentConfig;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use agent_runtime::{ApprovalDecision, TurnEvent};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = AgentRuntime::from_config(config)?;

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

async fn run_console(runtime: AgentRuntime) -> Result<()> {
    let session_id = runtime.default_session_id().to_string();
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::stdout();

    println!("agentd console attached to session `{session_id}`");

    loop {
        stdout.write_all(b"daemon-you> ").await?;
        stdout.flush().await?;

        let Some(line) = lines.next_line().await? else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if matches!(trimmed, "/exit" | "/quit") {
            break;
        }
        let output = runtime
            .chat_with_approval(&session_id, trimmed, |_request| async move {
                Ok(ApprovalDecision {
                    approved: true,
                    reason: Some("auto-approved in local daemon console".to_string()),
                })
            })
            .await?;
        render_events(&output.events);
        println!("agent> {}", output.final_response);
    }

    Ok(())
}

fn render_events(events: &[TurnEvent]) {
    for event in events {
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
            TurnEvent::AssistantMessage { .. }
            | TurnEvent::ApprovalRequested { .. }
            | TurnEvent::TurnCompleted { .. } => {}
        }
    }
}
