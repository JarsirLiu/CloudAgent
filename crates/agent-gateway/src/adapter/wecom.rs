use agent_app_server_client::AppServerClient;
use agent_protocol::TurnPolicy;
use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct WecomAdapterConfig {
    pub bot_id: String,
    pub bot_secret: String,
}

impl WecomAdapterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.bot_id.trim().is_empty() {
            anyhow::bail!("missing bot_id")
        }
        if self.bot_secret.trim().is_empty() {
            anyhow::bail!("missing bot_secret")
        }
        Ok(())
    }
}

pub struct PlatformRuntime;

impl PlatformRuntime {
    pub async fn wait(self) -> Result<()> {
        anyhow::bail!("wecom runtime is not implemented yet")
    }
}

pub fn spawn_runtime(
    config: WecomAdapterConfig,
    _node_client: AppServerClient,
    _turn_policy: TurnPolicy,
) -> Result<PlatformRuntime> {
    config.validate()?;
    Ok(PlatformRuntime)
}
