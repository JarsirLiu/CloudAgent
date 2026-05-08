mod node;

use anyhow::Result;
use std::ffi::OsString;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("local-app-server") => node::run_local_app_server(&args[1..]).await,
        _ => {
            tracing::info!(
                "gatewayd local node bootstrap ready: {}",
                agent_gateway::crate_name()
            );
            tracing::info!(
                "run `gatewayd local-app-server --conversation <id>` to start a local node app server"
            );
            Ok(())
        }
    }
}
