mod node;

use anyhow::Result;
use std::ffi::OsString;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt::init();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("serve") => node::run_resident_node(&args[1..]).await,
        _ => node::run_resident_node(&args).await,
    }
}
