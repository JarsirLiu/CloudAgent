use agent_app_server::run_stdio_server;
use agent_core::AgentHost;
use anyhow::Result;
use cli::agent_host::build_agent_host;
use cli::{ConsoleBootstrap, ConsoleConfig, run_console};
use config::AgentConfig;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = build_agent_host(config)?;
    runtime.run_startup_retention_cleanup().await;

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("console") => {
            run_console_mode(runtime).await?;
            return Ok(());
        }
        Some("app-server-stdio") => {
            let conversation_id =
                parse_conversation_id(&args).unwrap_or(runtime.ensure_active_conversation().await?);
            run_stdio_server(runtime, conversation_id, false, None).await?;
            return Ok(());
        }
        _ => {}
    }

    tracing::info!(
        "agentd is a worker-oriented binary; use `app-server-stdio` for node-managed workers"
    );
    tracing::info!("`console` remains available for development only");
    Ok(())
}

async fn run_console_mode(runtime: Arc<AgentHost>) -> Result<()> {
    let conversation_id = runtime.ensure_active_conversation().await?;
    let workspace_root = std::env::current_dir()?;
    run_console(ConsoleConfig {
        conversation_id: conversation_id.clone(),
        workspace_root,
        conversation_store_dir: runtime.conversation_store_dir().to_path_buf(),
        initial_filter_enabled: runtime.cli_pre_llm_filter_enabled(),
        initial_permission_mode: runtime.cli_permission_mode().to_string(),
        auto_approve: true,
        auto_approve_reason: Some("auto-approved in local daemon console".to_string()),
        target_label: "embedded".to_string(),
        bootstrap: ConsoleBootstrap::Embedded { runtime },
    })
    .await
}

fn parse_conversation_id(args: &[String]) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == "--conversation")
        .map(|pair| pair[1].clone())
}
