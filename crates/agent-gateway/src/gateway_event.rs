use crate::message::ReplyContext;
use agent_core::{TranscriptItem, TurnItemKind};

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
        item_id: String,
        call_id: Option<String>,
        kind: TurnItemKind,
        title: Option<String>,
    },
    ItemDelta {
        target: OutboundTarget,
        turn_id: String,
        item_id: String,
        call_id: Option<String>,
        kind: GatewayItemDeltaKind,
        delta: String,
    },
    ItemCompleted {
        target: OutboundTarget,
        turn_id: String,
        call_id: Option<String>,
        item: TranscriptItem,
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
