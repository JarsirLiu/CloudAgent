mod node;

use anyhow::Result;
use std::ffi::OsString;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("serve") => node::run_resident_node(&args[1..]).await,
        _ => {
            tracing::info!(
                "gatewayd local node bootstrap ready: {}",
                agent_gateway::crate_name()
            );
            tracing::info!(
                "run `gatewayd serve --listen 127.0.0.1:47070` to start the resident local node"
            );
            Ok(())
        }
    }
}
