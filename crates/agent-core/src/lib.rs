pub mod conversation;
pub mod context;
pub mod core;
pub mod events;
pub mod memory;
pub mod plan;
pub mod policy;
pub mod tool;
pub mod projection;
pub mod turn;

pub use agent_protocol::{
    ConversationSnapshot, ConversationStatus, ServerRequest, ServerRequestDecision, TurnEvent,
    TurnId, TurnState, UserTurnInput,
};
pub use conversation::{
    ActiveConversationTurn, ConversationHistory, ConversationMessage, ConversationState,
    PendingConversationRequest, PersistedConversation,
};
pub use context::{AgentContext, ContextManager, ModelContext, ToolExecutionContext};
pub use core::{ChatModel, ModelRequest, ModelResponse};
pub use events::{classify_notification, classify_turn_event, EventDelivery, EventStream};
pub use policy::ExecutionPolicy;
pub use projection::history_entry_from_message;
pub use tool::{ToolCall, ToolEvent, ToolExecutor, ToolResult, ToolSpec};
pub use turn::{AgentTurnOutput, TurnLifecycleClass, TurnLifecyclePhase};

pub fn crate_name() -> &'static str {
    "agent-core"
}
