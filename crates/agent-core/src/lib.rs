pub mod conversation;
pub mod context;
pub mod core;
pub mod events;
pub mod memory;
pub mod plan;
pub mod policy;
pub mod protocol;
pub mod tool;
pub mod projection;
pub mod turn;

pub use conversation::{
    ActiveConversationTurn, ConversationHistory, ConversationMessage, ConversationState, HistoryEntry,
    PendingConversationRequest, PersistedConversation, ThreadItem,
};
pub use context::{AgentContext, ContextManager, ModelContext, ToolExecutionContext};
pub use core::{ChatModel, ModelRequest, ModelResponse};
pub use events::{
    classify_turn_event, core_transcript_event_from_turn_event, CoreTranscriptEvent,
    EventDelivery, EventStream,
};
pub use policy::ExecutionPolicy;
pub use protocol::RequestId;
pub use projection::{
    agent_turn_output_from_events, history_entry_from_message, tool_events_from_turn_events,
};
pub use tool::{
    CommandExecutionStatus, StructuredToolResult, ToolCall, ToolEvent, ToolExecutor, ToolResult,
    ToolSpec, WriteFileStatus,
};
pub use turn::{
    AgentTurnOutput, ServerRequest, ServerRequestDecision, ToolApprovalRequest, TurnEvent, TurnId,
    TurnItemDeltaKind, TurnItemKind, TurnLifecycleClass, TurnLifecyclePhase, TurnState,
};

pub fn crate_name() -> &'static str {
    "agent-core"
}
