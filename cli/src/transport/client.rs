use crate::app::{ConsoleBootstrap, ConsoleConfig};
use agent_app_server_client::{AppServerClient, InProcessClientConfig, StdioClientConfig};
use anyhow::Result;

pub(crate) async fn create_client(
    config: &ConsoleConfig,
    conversation_id: String,
) -> Result<AppServerClient> {
    match &config.bootstrap {
        ConsoleBootstrap::Embedded { runtime } => {
            Ok(AppServerClient::in_process(InProcessClientConfig {
                runtime: runtime.clone(),
                conversation_id,
                auto_approve: config.auto_approve,
                auto_approve_reason: config.auto_approve_reason.clone(),
            }))
        }
        ConsoleBootstrap::WorkerStdio { program, args } => {
            AppServerClient::stdio(StdioClientConfig {
                program: program.clone(),
                args: args.clone(),
            })
            .await
        }
    }
}
