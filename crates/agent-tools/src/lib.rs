pub mod impls;
pub mod policy;
pub mod registry;
pub mod selection;
pub mod spec;

pub use registry::ToolRegistry;

pub fn crate_name() -> &'static str {
    "agent-tools"
}
