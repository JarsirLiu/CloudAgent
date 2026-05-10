use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeishuAdapterConfig {
    pub app_id: String,
    pub app_secret: String,
    pub domain: String,
    pub enable_cards: bool,
    pub thread_isolation: bool,
}

impl FeishuAdapterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.app_id.trim().is_empty() {
            bail!("feishu app_id cannot be empty");
        }
        if self.app_secret.trim().is_empty() {
            bail!("feishu app_secret cannot be empty");
        }
        if self.domain.trim().is_empty() {
            bail!("feishu domain cannot be empty");
        }
        Ok(())
    }
}

impl Default for FeishuAdapterConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            app_secret: String::new(),
            domain: "https://open.feishu.cn".to_string(),
            enable_cards: true,
            thread_isolation: true,
        }
    }
}
