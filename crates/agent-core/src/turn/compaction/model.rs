use crate::context::CompactionSummary;
use crate::conversation::ResponseItem;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionReason {
    UserRequested,
    ContextLimit,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPhase {
    StandaloneTurn,
    PreTurn,
    MidTurn,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InitialContextInjection {
    DoNotInject,
    BeforeLastRealUserMessage,
}

#[derive(Debug, Clone)]
pub struct CompactionRequest {
    pub conversation_id: String,
    pub turn_id: String,
    pub trigger: CompactionTrigger,
    pub reason: CompactionReason,
    pub phase: CompactionPhase,
    pub estimated_total_tokens: Option<usize>,
    pub minimum_history_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct CompactionOutcome {
    pub summary: CompactionSummary,
    pub rendered_summary: String,
    pub replacement_history: Vec<ResponseItem>,
    pub trigger: CompactionTrigger,
    pub reason: CompactionReason,
    pub phase: CompactionPhase,
    pub pre_context_tokens_estimate: u64,
    pub post_context_tokens_estimate: u64,
    pub pre_message_count: usize,
    pub post_message_count: usize,
    pub preserved_user_count: usize,
}
