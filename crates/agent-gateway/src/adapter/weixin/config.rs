use anyhow::Result;

const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

#[derive(Debug, Clone)]
pub struct WeixinAdapterConfig {
    pub account_id: String,
    pub token: String,
    pub base_url: String,
}

impl Default for WeixinAdapterConfig {
    fn default() -> Self {
        Self {
            account_id: String::new(),
            token: String::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }
}

impl WeixinAdapterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.account_id.trim().is_empty() {
            anyhow::bail!("weixin account_id is required")
        }
        if self.token.trim().is_empty() {
            anyhow::bail!("weixin token is required")
        }
        if self.base_url.trim().is_empty() {
            anyhow::bail!("weixin base_url is required")
        }
        Ok(())
    }
}
