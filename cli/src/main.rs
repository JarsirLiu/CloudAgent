use agent_runtime::AgentRuntime;
use anyhow::Result;
use cli::{ConsoleBanner, ConsoleConfig, run_console};
use config::AgentConfig;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = Arc::new(AgentRuntime::from_config(config)?);
    let session_id = runtime.default_session_id().to_string();

    run_console(
        runtime,
        ConsoleConfig {
            session_id: session_id.clone(),
            banner: ConsoleBanner::cli(&session_id),
            auto_approve: false,
            auto_approve_reason: None,
        },
    )
    .await
}
