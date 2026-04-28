mod controller;
mod event;
mod input;
mod render;
mod state;

use agent_runtime::AgentRuntime;
use anyhow::Result;
use std::sync::Arc;

pub use render::ConsoleBanner;

#[derive(Clone, Debug)]
pub struct ConsoleConfig {
    pub session_id: String,
    pub banner: ConsoleBanner,
    pub auto_approve: bool,
    pub auto_approve_reason: Option<String>,
}

pub async fn run_console(runtime: Arc<AgentRuntime>, config: ConsoleConfig) -> Result<()> {
    controller::run(runtime, config).await
}
