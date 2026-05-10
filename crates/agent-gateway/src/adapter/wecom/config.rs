use anyhow::{Result, bail};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WecomAdapterConfig {
    pub bot_id: String,
    pub bot_secret: String,
}

impl WecomAdapterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.bot_id.trim().is_empty() {
            bail!("wecom websocket bot_id cannot be empty");
        }
        if self.bot_secret.trim().is_empty() {
            bail!("wecom websocket bot_secret cannot be empty");
        }
        Ok(())
    }
}
