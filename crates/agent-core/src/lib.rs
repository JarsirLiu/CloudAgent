pub mod context;
pub mod core;
pub mod memory;
pub mod plan;
pub mod policy;
pub mod protocol;
pub mod runtime;
pub mod session;
pub mod task;
pub mod tool;

pub use context::{AgentContext, ToolExecutionContext};
pub use core::{ChatModel, ModelRequest, ModelResponse};
pub use policy::ExecutionPolicy;
pub use protocol::{
    ApprovalDecision, ApprovalRequest, SessionSnapshot, SessionState, TurnEvent, TurnId,
    TurnOutcome, TurnState, UserTurnInput,
};
pub use runtime::AgentTurnOutput;
pub use session::{AgentSession, ConversationMessage};
pub use tool::{ToolCall, ToolEvent, ToolExecutor, ToolResult, ToolSpec};

pub fn crate_name() -> &'static str {
    "agent-core"
}
