pub(crate) mod command_access;
pub mod impls;
pub mod policy;
pub mod registry;
pub mod spec;

pub use registry::{
    McpToolClient, McpToolDescriptor, McpToolInvocation, McpToolResponse, ToolRegistry,
    ToolRegistryOptions,
};

pub fn crate_name() -> &'static str {
    "agent-tools"
}
