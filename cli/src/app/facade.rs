use crate::app::core::types::{ConsoleConfig, TuiApp};
use crate::app::runtime::r#loop as runtime_loop;
use crate::state::reducer::ServerAction;
use crate::transport::client::create_client;
use agent_app_server_client::AppServerClient;
use anyhow::{Result, anyhow};
use std::io::{self, IsTerminal as _};
use tokio::time::{Duration, timeout};

const STARTUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn run_console(config: ConsoleConfig) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("cloudagent cli requires an interactive terminal");
    }
    run_tui_console(config).await
}

async fn run_tui_console(config: ConsoleConfig) -> Result<()> {
    let conversation_id = config.conversation_id.clone();
    let mut client = create_client(&config, conversation_id.clone()).await?;
    let mut app = TuiApp::new(
        conversation_id.clone(),
        &config.target_label,
        config.workspace_root.clone(),
        config.conversation_store_dir.clone(),
        config.initial_filter_enabled,
        config.initial_permission_mode.clone(),
    );
    load_initial_history(&client, &mut app, &conversation_id).await?;
    load_initial_skills(&client, &mut app).await?;
    runtime_loop::run_tui_event_loop(&mut app, &mut client).await?;
    client.shutdown().await
}

async fn load_initial_history(
    client: &AppServerClient,
    app: &mut TuiApp,
    conversation_id: &str,
) -> Result<()> {
    let response = timeout(
        STARTUP_REQUEST_TIMEOUT,
        client.request_conversation_history_typed(conversation_id),
    )
    .await
    .map_err(|_| {
        anyhow!(
            "timed out loading conversation history for `{conversation_id}`; the local node could not get a healthy worker response. restart `node` and retry"
        )
    })??;
    crate::app::conversation::actions::execute_server_action(
        app,
        ServerAction::ReplaceHistory(response.turns),
    );
    let status = timeout(
        STARTUP_REQUEST_TIMEOUT,
        client.request_conversation_status_typed(conversation_id),
    )
    .await
    .map_err(|_| {
        anyhow!(
            "timed out loading conversation status for `{conversation_id}`; the local node could not get a healthy worker response. restart `node` and retry"
        )
    })??;
    let mode = match status.snapshot.conversation_status {
        agent_core::ConversationStatus::Busy => agent_protocol::FrontendMode::Running,
        agent_core::ConversationStatus::Idle => agent_protocol::FrontendMode::Idle,
    };
    crate::app::conversation::actions::execute_server_action(
        app,
        ServerAction::SetFrontendMode(mode),
    );
    Ok(())
}

async fn load_initial_skills(client: &AppServerClient, app: &mut TuiApp) -> Result<()> {
    let response = timeout(STARTUP_REQUEST_TIMEOUT, client.request_skills_list_typed())
        .await
        .map_err(|_| anyhow!("timed out loading initial skills list"))??;
    app.bottom_pane.set_available_skills(response.skills);
    Ok(())
}
