use agent_app::{ConsoleBanner, ConsoleConfig, run_console};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use config::AgentConfig;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = Arc::new(AgentRuntime::from_config(config)?);

    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|arg| arg == "console") {
        run_console_mode(runtime).await?;
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

async fn run_console_mode(runtime: Arc<AgentRuntime>) -> Result<()> {
    let session_id = runtime.default_session_id().to_string();
    run_console(
        runtime,
        ConsoleConfig {
            session_id: session_id.clone(),
            banner: ConsoleBanner::daemon(&session_id),
            auto_approve: true,
            auto_approve_reason: Some("auto-approved in local daemon console".to_string()),
        },
    )
    .await
}
