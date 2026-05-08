use crate::app::{ConsoleBootstrap, ConsoleConfig};
use agent_app_server_client::{
    AppServerClient, InProcessClientConfig, LocalNodeClientConfig, StdioClientConfig,
};
use anyhow::{Result, anyhow};
use std::process::Stdio;
use std::time::Duration;

pub(crate) async fn create_client(
    config: &ConsoleConfig,
    conversation_id: String,
) -> Result<AppServerClient> {
    match &config.bootstrap {
        ConsoleBootstrap::LocalNode {
            address,
            program,
            args,
        } => create_local_node_client(address, program, args).await,
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

async fn create_local_node_client(
    address: &str,
    program: &std::ffi::OsString,
    args: &[std::ffi::OsString],
) -> Result<AppServerClient> {
    match AppServerClient::local_node(LocalNodeClientConfig {
        address: address.to_string(),
    })
    .await
    {
        Ok(client) => Ok(client),
        Err(first_error) => {
            std::process::Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            tokio::time::sleep(Duration::from_millis(250)).await;
            AppServerClient::local_node(LocalNodeClientConfig {
                address: address.to_string(),
            })
            .await
            .map_err(|second_error| {
                anyhow!(
                    "failed to connect to local node at {address}; initial error: {first_error}; retry after launching node failed: {second_error}"
                )
            })
        }
    }
}
