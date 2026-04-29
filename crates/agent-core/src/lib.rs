pub mod conversation;
pub mod context;
pub mod core;
pub mod history;
pub mod memory;
pub mod plan;
pub mod policy;
pub mod runtime;
pub mod task;
pub mod tool;

pub use agent_protocol::{
    ConversationSnapshot, ConversationStatus, ServerRequest, ServerRequestDecision, TurnEvent,
    TurnId, TurnState, UserTurnInput,
};
pub use conversation::{
    ActiveConversationTurn, ConversationState, PendingConversationRequest, PersistedConversation,
};
pub use context::{AgentContext, ToolExecutionContext};
pub use core::{ChatModel, ModelRequest, ModelResponse};
pub use history::{ConversationHistory, ConversationMessage};
pub use policy::ExecutionPolicy;
pub use runtime::AgentTurnOutput;
pub use tool::{ToolCall, ToolEvent, ToolExecutor, ToolResult, ToolSpec};

pub fn crate_name() -> &'static str {
    "agent-core"
}
