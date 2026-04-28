fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!(
        "gatewayd bootstrap placeholder: {}",
        agent_gateway::crate_name()
    );
    Ok(())
}
