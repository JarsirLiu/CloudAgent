pub mod direct;
mod message;
mod outbound;
mod runtime;

pub mod adapter;

pub use adapter::GatewayAdapter;
pub use direct::{app_server_message_to_outbound, gateway_message_to_command};
pub use message::GatewayMessage;
pub use outbound::{
    GatewayApprovalRequest, GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate,
};
pub use runtime::default_poll_interval;

pub fn crate_name() -> &'static str {
    "agent-gateway"
}
