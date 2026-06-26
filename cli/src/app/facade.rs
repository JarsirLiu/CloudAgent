use crate::app::core::types::ConsoleConfig;
use crate::app::session::run_console_session;
use anyhow::Result;
use std::io::{self, IsTerminal as _};

pub async fn run_console(config: ConsoleConfig) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("cloudagent cli requires an interactive terminal");
    }
    run_console_session(config).await
}
