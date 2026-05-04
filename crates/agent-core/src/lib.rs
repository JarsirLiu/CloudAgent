pub mod context;
pub mod conversation;
pub mod model;
pub mod projection;
pub mod rollout;
pub mod tool;
pub mod turn;

pub use context::{
    AgentContext, CompactionSummary, ContextCompactionConfig, ContextCompactionPlan,
    ContextCompactionResult, ContextFragment, ContextInputFilterService, ContextManager,
    ContextFacade, EnvironmentContext, FilterPolicy, ModelContext, ToolExecutionContext, apply_history_compaction,
    build_compaction_summary_request, plan_history_compaction, plan_manual_history_compaction,
};
pub use conversation::{
    ActiveConversationTurn, ConversationHistory, ConversationState, ConversationTurn,
    PendingConversationRequest, ResponseItem, TranscriptItem,
};
pub use model::{ChatModel, ModelRequest, ModelResponse, ModelUsage};
pub use projection::{
    ConversationHistoryBuilder, CoreTranscriptEvent, EventDelivery, EventStream, TranscriptBuilder,
    agent_turn_output_from_events, build_turns_from_rollout_items, classify_event_msg,
    conversation_history_from_rollout_items, core_transcript_event_from_event_msg,
    flatten_conversation_turns, tool_events_from_turn_events, transcript_item_from_response_item,
    transcript_items_from_response_items, transcript_items_from_rollout_items,
};
pub use rollout::RolloutItem;
pub use tool::{
    CommandExecutionStatus, ReadFileEntry, ReadFileStatus, SearchWorkspaceHit,
    SearchWorkspaceMode, SearchWorkspaceOperation, SearchWorkspaceStatus, StructuredToolResult,
    TaskKind, ToolCall, ToolEvent, ToolExecutor, ToolMode, ToolOutputDelta, ToolOutputStream,
    ToolResult, ToolSpec, ToolSurface, WriteFileStatus,
};
pub use turn::{
    AgentTurnOutput, EventMsg, ExecutionPolicy, RequestId, ServerRequest, ServerRequestDecision,
    ServerRequestDecisionKind, ToolApprovalRequest, TurnId, TurnItemDeltaKind, TurnItemKind,
    TurnLifecycleClass, TurnLifecyclePhase, TurnState,
};

pub fn crate_name() -> &'static str {
    "agent-core"
}
