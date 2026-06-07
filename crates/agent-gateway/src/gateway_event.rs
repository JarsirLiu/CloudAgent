use crate::message::ReplyContext;
use agent_core::{
    CompactionContinuation, ModelRetryStage, ModelUsage, ServerRequest, ServerRequestDecision,
    TranscriptItem,
};
use agent_protocol::RequestId;

#[derive(Debug, Clone)]
pub struct OutboundTarget {
    pub conversation_id: String,
    pub chat_id: String,
    pub chat_type: Option<String>,
    pub is_reply_chain: bool,
    pub reply_context: Option<ReplyContext>,
}

#[derive(Debug, Clone)]
pub enum GatewayItemDeltaKind {
    AgentMessage,
    Plan,
    ReasoningSummary,
    ReasoningText,
    CommandExecutionOutput,
    ToolOutput,
    FileChangeOutput,
}

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    TurnStarted {
        target: OutboundTarget,
        turn_id: String,
    },
    ItemStarted {
        target: OutboundTarget,
        turn_id: String,
        call_id: Option<String>,
        item: TranscriptItem,
    },
    ItemDelta {
        target: OutboundTarget,
        turn_id: String,
        item_id: String,
        call_id: Option<String>,
        kind: GatewayItemDeltaKind,
        segment_index: Option<usize>,
        delta: String,
    },
    ReasoningSummaryPartAdded {
        target: OutboundTarget,
        turn_id: String,
        item_id: String,
        summary_index: usize,
    },
    ItemCompleted {
        target: OutboundTarget,
        turn_id: String,
        call_id: Option<String>,
        item: TranscriptItem,
    },
    ServerRequestRequested {
        target: OutboundTarget,
        turn_id: String,
        request: ServerRequest,
    },
    ServerRequestResolved {
        target: OutboundTarget,
        turn_id: String,
        request_id: RequestId,
        request: ServerRequest,
        decision: ServerRequestDecision,
    },
    TokenUsageUpdated {
        target: OutboundTarget,
        turn_id: String,
        last_usage: ModelUsage,
        total_usage: ModelUsage,
        model_context_window: Option<u64>,
    },
    ModelRetrying {
        target: OutboundTarget,
        turn_id: String,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    },
    ContextCompactionStarted {
        target: OutboundTarget,
        turn_id: Option<String>,
        continuation: CompactionContinuation,
        estimated_tokens: u64,
    },
    ContextCompacted {
        target: OutboundTarget,
        turn_id: Option<String>,
        continuation: CompactionContinuation,
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    TurnCompleted {
        target: OutboundTarget,
        turn_id: String,
    },
    TurnFailed {
        target: OutboundTarget,
        turn_id: String,
        error: String,
    },
    TurnCancelled {
        target: OutboundTarget,
        turn_id: String,
        reason: String,
    },
    Info {
        target: OutboundTarget,
        message: String,
    },
    Error {
        target: OutboundTarget,
        message: String,
    },
}
