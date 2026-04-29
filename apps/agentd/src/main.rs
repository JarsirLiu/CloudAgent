use agent_app_server::run_stdio_server;
use agent_runtime::AgentRuntime;
use anyhow::Result;
use cli::{ConsoleConfig, ConsoleConnection, run_console};
use config::AgentConfig;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = Arc::new(AgentRuntime::from_config(config)?);

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("console") => {
            run_console_mode(runtime).await?;
            return Ok(());
        }
        Some("app-server-stdio") => {
            let conversation_id = parse_conversation_id(&args)
                .unwrap_or_else(|| runtime.default_conversation_id().to_string());
            run_stdio_server(runtime, conversation_id, false, None).await?;
            return Ok(());
        }
        _ => {}
    }

    tracing::info!(
        "agentd ready; conversation store at {}",
        runtime.default_conversation_id()
    );
    tracing::info!("run `cargo run -p agentd -- console` to attach a local console");
    tokio::signal::ctrl_c().await?;
    Ok(())
}

async fn run_console_mode(runtime: Arc<AgentRuntime>) -> Result<()> {
    let conversation_id = runtime.default_conversation_id().to_string();
    run_console(ConsoleConfig {
        session_id: conversation_id.clone(),
        auto_approve: true,
        auto_approve_reason: Some("auto-approved in local daemon console".to_string()),
        connection: ConsoleConnection::InProcess { runtime },
    })
    .await
}

fn parse_conversation_id(args: &[String]) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == "--conversation")
        .map(|pair| pair[1].clone())
}
