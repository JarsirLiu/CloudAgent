mod manager;
mod tool_context;

pub use manager::{ContextManager, ModelContext};
pub use tool_context::{AgentContext, ToolExecutionContext};

pub fn module_name() -> &'static str {
    "agent-core::context"
}
