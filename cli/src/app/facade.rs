use crate::app::core::types::{ConsoleConfig, TuiApp};
use crate::app::runtime::r#loop as runtime_loop;
use crate::state::reducer::ServerAction;
use crate::transport::client::create_client;
use agent_app_server_client::AppServerClient;
use anyhow::Result;
use std::io::{self, IsTerminal as _};

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
    runtime_loop::run_tui_event_loop(&mut app, &mut client).await?;
    client.shutdown().await
}

async fn load_initial_history(
    client: &AppServerClient,
    app: &mut TuiApp,
    conversation_id: &str,
) -> Result<()> {
    let response = client
        .request_conversation_history_typed(conversation_id)
        .await?;
    crate::app::conversation::actions::execute_server_action(
        app,
        ServerAction::ReplaceHistory(response.turns),
    );
    let status = client
        .request_conversation_status_typed(conversation_id)
        .await?;
    let mode = match status.snapshot.conversation_status {
        agent_core::ConversationStatus::Busy => agent_protocol::FrontendMode::Running,
        agent_core::ConversationStatus::Idle => agent_protocol::FrontendMode::Idle,
    };
    crate::app::conversation::actions::execute_server_action(app, ServerAction::SetFrontendMode(mode));
    Ok(())
}
