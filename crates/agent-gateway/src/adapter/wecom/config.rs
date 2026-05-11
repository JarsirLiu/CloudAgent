use anyhow::Result;

#[derive(Debug, Clone, Default)]
pub struct WecomAdapterConfig {
    pub bot_id: String,
    pub bot_secret: String,
    pub dm_policy: WecomPolicy,
    pub group_policy: WecomPolicy,
    pub allow_from: Vec<String>,
    pub group_allow_from: Vec<String>,
}

impl WecomAdapterConfig {
    pub fn validate(&self) -> Result<()> {
        if self.bot_id.trim().is_empty() {
            anyhow::bail!("missing bot_id")
        }
        if self.bot_secret.trim().is_empty() {
            anyhow::bail!("missing bot_secret")
        }
        self.dm_policy.validate("dm_policy")?;
        self.group_policy.validate("group_policy")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WecomPolicy {
    #[default]
    Open,
    Allowlist,
    Disabled,
}

impl WecomPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "open" => Some(Self::Open),
            "allowlist" => Some(Self::Allowlist),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }

    pub fn validate(self, field: &str) -> Result<()> {
        match self {
            Self::Open | Self::Allowlist | Self::Disabled => Ok(()),
        }
        .map_err(|err: anyhow::Error| anyhow::anyhow!("{field}: {err}"))
    }
}
