use crate::app::core::types::{ConsoleConfig, TuiApp};
use crate::app::runtime::r#loop as runtime_loop;
use crate::transport::client::create_client;
use agent_app_server_client::AppServerClient;
use anyhow::{Result, anyhow};
use tokio::time::{Duration, timeout};

const STARTUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn run_console_session(config: ConsoleConfig) -> Result<()> {
    let conversation_id = config.conversation_id.clone();
    let mut client = create_client(&config, conversation_id.clone()).await?;
    let mut app = TuiApp::new(
        conversation_id,
        &config.target_label,
        config.workspace_root.clone(),
        config.conversation_store_dir.clone(),
        config.initial_filter_enabled,
        config.initial_permission_mode.clone(),
    );
    app.conversation_history_turn_limit = config.conversation_history_turn_limit;
    load_initial_skills(&client, &mut app).await?;
    runtime_loop::run_tui_event_loop(&mut app, &mut client).await?;
    client.shutdown().await
}

async fn load_initial_skills(client: &AppServerClient, app: &mut TuiApp) -> Result<()> {
    let response = timeout(STARTUP_REQUEST_TIMEOUT, client.request_skills_list_typed())
        .await
        .map_err(|_| anyhow!("timed out loading initial skills list"))??;
    app.bottom_pane.set_available_skills(response.skills);
    Ok(())
}
