use crate::app::core::types::{ConsoleConfig, TuiApp};
use crate::app::runtime::r#loop as runtime_loop;
use crate::transport::client::create_client;
use agent_protocol::AppClientCommand;
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
    client.send_command(AppClientCommand::RequestConversationHistory {
        conversation_id: conversation_id.clone(),
    })?;
    runtime_loop::run_tui_event_loop(&mut app, &mut client).await?;
    client.shutdown().await
}
