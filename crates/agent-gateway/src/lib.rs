pub mod adapter;
mod app_server_mapping;
mod config;
mod gateway_event;
mod message;
mod platform;
mod platforms;
mod runtime;
mod session;

pub use config::{GatewayConfig, GatewayConfigFile, LlmConfig, load_gateway_config};
pub use gateway_event::{GatewayEvent, GatewayItemDeltaKind, OutboundTarget};
pub use platform::{MessageHandler, PlatformAdapter};
pub use runtime::run_gateway;

pub fn crate_name() -> &'static str {
    "agent-gateway"
}
