use anyhow::Result;

mod node;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()))
        .init();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    node::run_resident_node(&args).await
}
