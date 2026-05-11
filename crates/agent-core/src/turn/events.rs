use super::compaction::CompactionContinuation;
use crate::conversation::{InputItem, TranscriptItem};
use crate::model::ModelUsage;
use crate::turn::RequestId;
use serde::{Deserialize, Serialize};

pub type TurnId = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandApprovalRequest {
    pub turn_id: TurnId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub command_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileChangeApprovalRequest {
    pub turn_id: TurnId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub change_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerRequestDecision {
    pub decision: ServerRequestDecisionKind,
    pub reason: Option<String>,
}

impl ServerRequestDecision {
    pub fn accept(reason: Option<String>) -> Self {
        Self {
            decision: ServerRequestDecisionKind::Accept,
            reason,
        }
    }

    pub fn accept_for_session(reason: Option<String>) -> Self {
        Self {
            decision: ServerRequestDecisionKind::AcceptForSession,
            reason,
        }
    }

    pub fn decline(reason: Option<String>) -> Self {
        Self {
            decision: ServerRequestDecisionKind::Decline,
            reason,
        }
    }

    pub fn cancel(reason: Option<String>) -> Self {
        Self {
            decision: ServerRequestDecisionKind::Cancel,
            reason,
        }
    }

    pub fn is_approved(&self) -> bool {
        matches!(
            self.decision,
            ServerRequestDecisionKind::Accept | ServerRequestDecisionKind::AcceptForSession
        )
    }

    pub fn label(&self) -> &'static str {
        match self.decision {
            ServerRequestDecisionKind::Accept => "approved",
            ServerRequestDecisionKind::AcceptForSession => "approved for session",
            ServerRequestDecisionKind::Decline => "denied",
            ServerRequestDecisionKind::Cancel => "cancelled",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServerRequestDecisionKind {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "request_type", rename_all = "snake_case")]
pub enum ServerRequest {
    CommandApproval { request: CommandApprovalRequest },
    FileChangeApproval { request: FileChangeApprovalRequest },
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Idle,
    Running,
    WaitingForServerRequest,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemKind {
    UserMessage,
    AssistantMessage,
    CommandExecution,
    FileChange,
    ToolCall,
    ToolResult,
    Reasoning,
    SystemNote,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemDeltaKind {
    Text,
    CommandExecutionOutput,
    ToolOutput,
    FileChangeOutput,
    ReasoningText,
    ReasoningSummary,
    JsonPatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventMsg {
    TurnStarted {
        turn_id: TurnId,
        conversation_id: String,
        user_input: Vec<InputItem>,
    },
    ModelRequestStarted {
        turn_id: TurnId,
        message_count: usize,
        tool_count: usize,
    },
    ModelResponseReceived {
        turn_id: TurnId,
        model_name: Option<String>,
        has_content: bool,
        tool_call_count: usize,
    },
    ModelRetrying {
        turn_id: TurnId,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    },
    TokenUsageUpdated {
        turn_id: TurnId,
        last_usage: ModelUsage,
        total_usage: ModelUsage,
        model_context_window: Option<u64>,
        request_estimated_tokens: u64,
    },
    ContextCompacted {
        turn_id: TurnId,
        continuation: CompactionContinuation,
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    ContextCompactionStarted {
        turn_id: TurnId,
        continuation: CompactionContinuation,
        estimated_tokens: u64,
    },
    ItemStarted {
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        kind: TurnItemKind,
        title: Option<String>,
    },
    ItemDelta {
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        kind: TurnItemDeltaKind,
        segment_index: Option<usize>,
        delta: String,
    },
    ItemCompleted {
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        item: TranscriptItem,
    },
    ServerRequestRequested {
        turn_id: TurnId,
        request: ServerRequest,
    },
    ServerRequestResolved {
        turn_id: TurnId,
        request: ServerRequest,
        decision: ServerRequestDecision,
    },
    TurnCompleted {
        turn_id: TurnId,
    },
    TurnFailed {
        turn_id: TurnId,
        error: String,
    },
    TurnCancelled {
        turn_id: TurnId,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelRetryStage {
    Request,
    Streaming,
}

#[derive(Clone, Debug)]
pub struct PendingTurnRequest {
    pub request_id: RequestId,
    pub request: ServerRequest,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_started_preserves_structured_user_input() {
        let value = serde_json::json!({
            "type": "turn_started",
            "turn_id": "turn-1",
            "conversation_id": "default",
            "user_input": [
                { "type": "text", "text": "look at this" },
                {
                    "type": "image",
                    "source": {
                        "type": "remote_url",
                        "url": "https://example.com/diagram.png"
                    },
                    "detail": "high",
                    "alt": "diagram"
                }
            ]
        });

        let parsed: EventMsg = serde_json::from_value(value.clone()).expect("parse event");
        let reserialized = serde_json::to_value(parsed).expect("serialize event");

        assert_eq!(reserialized, value);
    }
}
