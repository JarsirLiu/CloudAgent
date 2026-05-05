mod config;
mod error;
mod event;
mod providers;
mod request;

pub use config::ProviderRuntimeConfig;
pub use error::{ProviderRequestError, ProviderStreamError};
pub use event::{ProviderCompletion, ProviderMetadata, ProviderStreamEvent, ProviderToolCallDelta};
pub use providers::openai_compatible::OpenAiCompatibleModel;

pub fn crate_name() -> &'static str {
    "agent-model-provider"
}
