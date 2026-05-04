use config::LlmConfig;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ProviderRuntimeConfig {
    pub stream_idle_timeout: Duration,
    pub request_max_retries: u64,
    pub stream_max_retries: u64,
}

impl From<&LlmConfig> for ProviderRuntimeConfig {
    fn from(value: &LlmConfig) -> Self {
        Self {
            stream_idle_timeout: Duration::from_millis(value.stream_idle_timeout_ms.max(1_000)),
            request_max_retries: value.request_max_retries,
            stream_max_retries: value.stream_max_retries,
        }
    }
}
