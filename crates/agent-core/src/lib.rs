pub mod context;
pub mod conversation;
pub mod core;
pub mod events;
pub mod memory;
pub mod plan;
pub mod policy;
pub mod projection;
pub mod protocol;
pub mod rollout;
pub mod tool;
pub mod turn;

pub use context::{AgentContext, ContextManager, ModelContext, ToolExecutionContext};
pub use conversation::{
    ActiveConversationTurn, ConversationHistory, ConversationState, PendingConversationRequest,
    PersistedConversation, ResponseItem, TranscriptItem,
};
pub use core::{ChatModel, ModelRequest, ModelResponse};
pub use events::{
    CoreTranscriptEvent, EventDelivery, EventStream, classify_event_msg,
    core_transcript_event_from_event_msg,
};
pub use policy::ExecutionPolicy;
pub use projection::{
    TranscriptBuilder, agent_turn_output_from_events, tool_events_from_turn_events,
    transcript_item_from_response_item, transcript_items_from_response_items,
    transcript_items_from_rollout_items,
};
pub use protocol::RequestId;
pub use rollout::RolloutItem;
pub use tool::{
    CommandExecutionStatus, StructuredToolResult, ToolCall, ToolEvent, ToolExecutor, ToolResult,
    ToolSpec, WriteFileStatus,
};
pub use turn::{
    AgentTurnOutput, EventMsg, ServerRequest, ServerRequestDecision, ToolApprovalRequest, TurnId,
    TurnItemDeltaKind, TurnItemKind, TurnLifecycleClass, TurnLifecyclePhase, TurnState,
};

pub fn crate_name() -> &'static str {
    "agent-core"
}
