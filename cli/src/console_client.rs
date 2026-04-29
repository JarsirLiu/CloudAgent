use crate::console::{ConsoleConfig, ConsoleConnection};
use agent_app_server_client::{AppServerClient, InProcessClientConfig, StdioClientConfig};
use anyhow::Result;

pub(crate) async fn create_client(
    config: &ConsoleConfig,
    session_id: String,
) -> Result<AppServerClient> {
    match &config.connection {
        ConsoleConnection::InProcess { runtime } => {
            Ok(AppServerClient::in_process(InProcessClientConfig {
                runtime: runtime.clone(),
                session_id,
                auto_approve: config.auto_approve,
                auto_approve_reason: config.auto_approve_reason.clone(),
            }))
        }
        ConsoleConnection::Stdio { program, args } => {
            AppServerClient::stdio(StdioClientConfig {
                program: program.clone(),
                args: args.clone(),
            })
            .await
        }
    }
}

