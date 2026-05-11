mod client;
mod config;
mod inbound;
mod outbound;
mod runtime;

pub use config::WeixinAdapterConfig;
pub use runtime::{PlatformRuntime, spawn_runtime};
