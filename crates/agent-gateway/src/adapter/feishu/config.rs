use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct FeishuAdapterConfig {
    pub app_id: String,
    pub app_secret: String,
    pub domain: String,
    pub verification_token: Option<String>,
    pub encrypt_key: Option<String>,
    pub enable_cards: bool,
    pub thread_isolation: bool,
    pub reply_to_trigger: bool,
    pub group_only_mentioned: bool,
    pub group_reply_without_mention: bool,
}

impl FeishuAdapterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.app_id.trim().is_empty() {
            anyhow::bail!("missing app_id")
        }
        if self.app_secret.trim().is_empty() {
            anyhow::bail!("missing app_secret")
        }
        Ok(())
    }
}
