use agent_app_server::run_stdio_server;
use agent_core::AgentHost;
use anyhow::Result;
use cli::agent_host::build_agent_host;
use cli::app::cli_settings::load_cli_settings;
use cli::{ConsoleBootstrap, ConsoleConfig, run_console};
use config::{AgentConfig, TerminalResizeReflowMaxRows};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let workspace_root = std::env::current_dir()?;
    let mut config = AgentConfig::load(workspace_root)?;
    apply_data_dir_override(&mut config, &args);
    if let Ok(Some(settings)) = load_cli_settings(&config.runtime.conversation_store_dir) {
        config.cli.pre_llm_filter_enabled = settings.pre_llm_filter_enabled;
        config.cli.permission_mode = settings.permission_mode;
    }
    let terminal_resize_reflow_max_rows = config.cli.terminal_resize_reflow_max_rows;
    let conversation_history_turn_limit = config.cli.conversation_history_turn_limit;
    let runtime = build_agent_host(config)?;

    match args.get(1).map(String::as_str) {
        Some("console") => {
            runtime.run_startup_retention_cleanup().await;
            run_console_mode(
                runtime,
                terminal_resize_reflow_max_rows,
                conversation_history_turn_limit,
            )
            .await?;
            return Ok(());
        }
        Some("app-server-stdio") => {
            run_stdio_server(runtime, false, None).await?;
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

async fn run_console_mode(
    runtime: Arc<AgentHost>,
    terminal_resize_reflow_max_rows: TerminalResizeReflowMaxRows,
    conversation_history_turn_limit: Option<usize>,
) -> Result<()> {
    let conversation_id = runtime.ensure_active_conversation().await?;
    let workspace_root = std::env::current_dir()?;
    run_console(ConsoleConfig {
        conversation_id: conversation_id.clone(),
        workspace_root,
        conversation_store_dir: runtime.conversation_store_dir().to_path_buf(),
        initial_filter_enabled: runtime.cli_pre_llm_filter_enabled(),
        initial_permission_mode: runtime.cli_permission_mode().to_string(),
        terminal_resize_reflow_max_rows,
        conversation_history_turn_limit,
        auto_approve: true,
        auto_approve_reason: Some("auto-approved in local daemon console".to_string()),
        target_label: "embedded".to_string(),
        bootstrap: ConsoleBootstrap::Embedded { runtime },
    })
    .await
}

fn apply_data_dir_override(config: &mut AgentConfig, args: &[String]) {
    let Some(value) = args
        .windows(2)
        .find(|pair| pair[0] == "--data-dir")
        .map(|pair| PathBuf::from(&pair[1]))
    else {
        return;
    };
    config.runtime.data_root_dir = if value.is_absolute() {
        value
    } else {
        config.workspace_root.join(value)
    };
    config.runtime.conversation_store_dir = config.runtime.data_root_dir.join("conversations");
    config.runtime.memory.root_dir = config.runtime.data_root_dir.join("state").join("memory");
}
