pub mod approval;
pub mod context;
pub mod conversation;
pub mod error;
pub mod host;
pub mod model;
pub mod observability;
pub mod output_truncation;
pub mod projection;
pub mod rollout;
pub mod runtime_item;
pub mod skill;
pub mod state;
pub mod tool;
pub mod turn;
pub mod web_search_presentation;

pub use approval::ApprovalGrantStoreBackend;
pub use context::{
    AgentContext, CompactionSummary, ContextCompactionConfig, ContextCompactionPlan,
    ContextCompactionResult, ContextFacade, ContextFragment, ContextInputFilterService,
    ContextManager, EnvironmentContext, FilterPolicy, ModelContext, ToolExecutionContext,
    apply_history_compaction, build_compaction_summary_request, plan_history_compaction,
    plan_manual_history_compaction,
};
pub use conversation::{
    ActiveConversationTurn, AttachmentRef, ConversationHistory, ConversationSnapshot,
    ConversationState, ConversationStatus, ConversationSummary, ConversationTurn, ImageDetail,
    InputItem, PendingConversationRequest, ResponseItem, TranscriptItem, input_items_are_blank,
    input_items_attachment_count, input_items_display_text, input_items_preview_text,
    input_items_text_len, input_items_to_plain_text, text_input_items, visible_message_count,
};
pub use error::TurnInterruptedError;
pub use host::{
    AgentHost, AgentHostExt, AgentHostParts, AgentMetadata, ConversationStoreBackend,
    MemoryBackend, RolloutRecorderBackend,
};
pub use model::{
    ChatModel, ChatModelFactory, ModelProviderSettings, ModelRequest, ModelResponse,
    ModelRetryDecision, ModelStreamObserver, ModelUsage, ReloadableChatModel, WebSearchAction,
    WebSearchRecord, await_server_request_decision, complete_model_request,
    complete_model_request_streaming,
};
pub use observability::{
    AuditEventEntry, ContextBudgetLogEntry, append_audit_event, append_audit_event_safe,
    append_context_budget_log, verify_audit_chain,
};
pub use projection::{
    ConversationHistoryBuilder, CoreTranscriptEvent, EventDelivery, EventStream, TranscriptBuilder,
    agent_turn_output_from_events, build_turns_from_rollout_items, classify_event_msg,
    core_transcript_event_from_event_msg, filter_history_ui_turn, filter_history_ui_turns,
    flatten_conversation_turns, tool_events_from_turn_events, transcript_item_from_response_item,
    transcript_items_from_response_items, transcript_items_from_rollout_items,
};
pub use rollout::{
    RolloutItem,
    policy::{RolloutPersistenceMode, persisted_rollout_item, persisted_rollout_items},
    reconstruction::conversation_history_from_rollout_items,
};
pub use runtime_item::{
    RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress, RuntimeItemSnapshot, RuntimeItemStatus,
};
pub use skill::{
    SkillCatalog, SkillDependencies, SkillDocument, SkillInvocationMode, SkillMetadata,
    SkillRuntime, SkillScaffoldOutcome, SkillScaffoldSpec, SkillScope, SkillValidationReport,
    create_skill_scaffold, render_skill_budget_summary, render_skill_injection,
    render_skill_summary_item, render_skill_template, render_truncated_skill_injection,
    validate_skill_dir,
};
pub use state::{ActiveTurnHandle, AgentState};
pub use tool::{
    ApprovalGrantKey, ApprovalRequirement, ChatTurnToolExposure, CommandExecutionStatus,
    DirectoryEntry, McpCallResult, ParallelToolInvocation, ParallelToolResult, ReadFileEntry,
    ReadFileStatus, ResolvedToolSet, SearchWorkspaceHit, SearchWorkspaceMode,
    SearchWorkspaceOperation, SearchWorkspaceStatus, StructuredToolResult, ToolBackend,
    ToolBatchExecutionStrategy, ToolCall, ToolEvent, ToolExecutionPolicy, ToolExecutor,
    ToolIdentity, ToolOutputDelta, ToolOutputKind, ToolOutputStream, ToolResult, ToolSearchHit,
    ToolSource, ToolSpec, WriteFileStatus, execute_tool_call_streaming,
    run_parallel_tool_invocations, summarize_arguments,
};
pub use turn::{
    AgentTurnOutput, ApprovalPolicy, CONVERSATION_BUSY_ERROR_CODE, CONVERSATION_BUSY_ERROR_MESSAGE,
    ChatTurnSettings, CommandApprovalRequest, CompactionOutcome, CompactionPhase, CompactionReason,
    CompactionRequest, CompactionTrigger, EventMsg, ExecutionPolicy, FileChangeApprovalRequest,
    InitialContextInjection, ManualCompactionOutcome, ModelRetryStage, PermissionProfile,
    RequestId, RequestTokenBaseline, RestoredTurnTokenState, ServerRequest, ServerRequestDecision,
    ServerRequestDecisionKind, ServerRequestHandler, TokenUsageState, ToolBatchOutcome, TurnHost,
    TurnId, TurnItemDeltaKind, TurnItemKind, TurnLifecycleClass, TurnLifecyclePhase, TurnOutcome,
    TurnPolicy, TurnState, UserTurnInput, chat, chat_with_approval, chat_with_approval_and_events,
    compact_conversation, conversation_busy_error, emit_assistant_message_item, emit_event,
    execute_chat_turn, next_turn_id, paginate_turns, run_manual_compaction, run_turn_with_approval,
};
pub use web_search_presentation::{
    WEB_SEARCH_TOOL_NAME, web_search_detail, web_search_runtime_item_completed,
    web_search_runtime_item_started, web_search_summary, web_search_transcript_item,
};

pub fn crate_name() -> &'static str {
    "agent-core"
}
