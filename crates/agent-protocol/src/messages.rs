use crate::types::{ConversationSnapshot, FrontendMode, NotificationDelivery, NotificationStream};
use crate::{
    ConversationSummary, ConversationTurn, ModelRetryStage, ModelUsage, RequestId, ServerRequest,
    ServerRequestDecision, TranscriptItem, TurnId, TurnItemKind, UserTurnInput,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppClientCommand {
    SubmitTurn(UserTurnInput),
    ResolveServerRequest {
        conversation_id: String,
        request_id: RequestId,
        decision: ServerRequestDecision,
    },
    InterruptTurn {
        conversation_id: String,
    },
    CompactConversation {
        conversation_id: String,
    },
    ResetConversation {
        conversation_id: String,
    },
    RequestConversationStatus {
        conversation_id: String,
    },
    RequestConversationHistory {
        conversation_id: String,
    },
    RequestConversationHistoryPage {
        conversation_id: String,
        before_turn_id: Option<String>,
        limit: usize,
    },
    ListConversations,
    SetConversationTitle {
        conversation_id: String,
        title: String,
    },
    CreateConversation {
        conversation_id: String,
    },
    SwitchConversation {
        conversation_id: String,
    },
    ArchiveConversation {
        conversation_id: String,
    },
    DeleteConversation {
        conversation_id: String,
    },
    SubscribeConversation {
        conversation_id: String,
    },
    UnsubscribeConversation {
        conversation_id: String,
    },
    Exit,
}

impl AppClientCommand {
    pub fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::SubmitTurn(input) => Some(&input.conversation_id),
            Self::ResolveServerRequest {
                conversation_id, ..
            }
            | Self::InterruptTurn { conversation_id }
            | Self::CompactConversation { conversation_id }
            | Self::ResetConversation { conversation_id }
            | Self::RequestConversationStatus { conversation_id }
            | Self::RequestConversationHistory { conversation_id }
            | Self::RequestConversationHistoryPage {
                conversation_id, ..
            }
            | Self::CreateConversation { conversation_id }
            | Self::SetConversationTitle {
                conversation_id, ..
            }
            | Self::SwitchConversation { conversation_id }
            | Self::ArchiveConversation { conversation_id }
            | Self::DeleteConversation { conversation_id }
            | Self::SubscribeConversation { conversation_id }
            | Self::UnsubscribeConversation { conversation_id } => Some(conversation_id),
            Self::ListConversations | Self::Exit => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerNotification {
    FrontendStateChanged {
        conversation_id: String,
        mode: FrontendMode,
    },
    TurnStarted {
        conversation_id: String,
        turn_id: TurnId,
    },
    ItemStarted {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        kind: TurnItemKind,
        title: Option<String>,
    },
    AgentMessageDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    PlanDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningSummaryTextDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningTextDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    CommandExecutionOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    ToolOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    FileChangeOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        call_id: Option<String>,
        delta: String,
    },
    TokenUsageUpdated {
        conversation_id: String,
        turn_id: TurnId,
        last_usage: ModelUsage,
        total_usage: ModelUsage,
        model_context_window: Option<u64>,
    },
    ModelRetrying {
        conversation_id: String,
        turn_id: TurnId,
        stage: ModelRetryStage,
        attempt: u64,
        next_delay_ms: u64,
    },
    ContextCompacted {
        conversation_id: String,
        turn_id: TurnId,
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    ContextCompactionStarted {
        conversation_id: String,
        turn_id: TurnId,
        estimated_tokens: u64,
    },
    ItemCompleted {
        conversation_id: String,
        turn_id: TurnId,
        call_id: Option<String>,
        item: TranscriptItem,
    },
    ServerRequestRequested {
        conversation_id: String,
        turn_id: TurnId,
        request: ServerRequest,
    },
    ServerRequestResolved {
        conversation_id: String,
        turn_id: TurnId,
        request_id: RequestId,
        request: ServerRequest,
        decision: ServerRequestDecision,
    },
    TurnCompleted {
        conversation_id: String,
        turn_id: TurnId,
    },
    TurnFailed {
        conversation_id: String,
        turn_id: TurnId,
        error: String,
    },
    TurnCancelled {
        conversation_id: String,
        turn_id: TurnId,
        reason: String,
    },
    ConversationStatus {
        conversation_id: String,
        snapshot: ConversationSnapshot,
    },
    ConversationHistory {
        conversation_id: String,
        turns: Vec<ConversationTurn>,
    },
    ConversationHistoryPage {
        conversation_id: String,
        turns: Vec<ConversationTurn>,
        has_more: bool,
        next_before_turn_id: Option<String>,
    },
    ConversationList {
        conversation_id: String,
        conversations: Vec<ConversationSummary>,
    },
    ConversationSwitched {
        conversation_id: String,
    },
    ConversationSubscriptionChanged {
        conversation_id: String,
        subscribed: bool,
    },
    Info {
        conversation_id: String,
        message: String,
    },
    Error {
        conversation_id: String,
        // Stable machine-readable error prefixes may be included in message text.
        // Current convention includes:
        // - ERR_CONVERSATION_BUSY: submitted turn rejected because conversation already has an active turn.
        message: String,
    },
}

impl AppServerNotification {
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::FrontendStateChanged {
                conversation_id, ..
            }
            | Self::TurnStarted {
                conversation_id, ..
            }
            | Self::ItemStarted {
                conversation_id, ..
            }
            | Self::AgentMessageDelta {
                conversation_id, ..
            }
            | Self::PlanDelta {
                conversation_id, ..
            }
            | Self::ReasoningSummaryTextDelta {
                conversation_id, ..
            }
            | Self::ReasoningTextDelta {
                conversation_id, ..
            }
            | Self::CommandExecutionOutputDelta {
                conversation_id, ..
            }
            | Self::ToolOutputDelta {
                conversation_id, ..
            }
            | Self::FileChangeOutputDelta {
                conversation_id, ..
            }
            | Self::TokenUsageUpdated {
                conversation_id, ..
            }
            | Self::ModelRetrying {
                conversation_id, ..
            }
            | Self::ContextCompacted {
                conversation_id, ..
            }
            | Self::ContextCompactionStarted {
                conversation_id, ..
            }
            | Self::ItemCompleted {
                conversation_id, ..
            }
            | Self::ServerRequestRequested {
                conversation_id, ..
            }
            | Self::ServerRequestResolved {
                conversation_id, ..
            }
            | Self::TurnCompleted {
                conversation_id, ..
            }
            | Self::TurnFailed {
                conversation_id, ..
            }
            | Self::TurnCancelled {
                conversation_id, ..
            }
            | Self::ConversationStatus {
                conversation_id, ..
            }
            | Self::ConversationHistory {
                conversation_id, ..
            }
            | Self::ConversationHistoryPage {
                conversation_id, ..
            }
            | Self::ConversationList {
                conversation_id, ..
            }
            | Self::ConversationSwitched {
                conversation_id, ..
            }
            | Self::ConversationSubscriptionChanged {
                conversation_id, ..
            }
            | Self::Info {
                conversation_id, ..
            }
            | Self::Error {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerRequest {
    ServerRequest {
        request_id: RequestId,
        conversation_id: String,
        request: ServerRequest,
    },
}

impl AppServerRequest {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::ServerRequest { request_id, .. } => request_id,
        }
    }

    pub fn conversation_id(&self) -> &str {
        match self {
            Self::ServerRequest {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppServerMessage {
    Notification(AppServerNotification),
    Request(AppServerRequest),
}

impl AppServerMessage {
    pub fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::Notification(notification) => Some(notification.conversation_id()),
            Self::Request(request) => Some(request.conversation_id()),
        }
    }
}

pub fn classify_notification(
    notification: &AppServerNotification,
) -> (NotificationStream, NotificationDelivery) {
    match notification {
        AppServerNotification::AgentMessageDelta { .. }
        | AppServerNotification::PlanDelta { .. }
        | AppServerNotification::ReasoningSummaryTextDelta { .. }
        | AppServerNotification::ReasoningTextDelta { .. }
        | AppServerNotification::ItemCompleted { .. }
        | AppServerNotification::TurnCompleted { .. } => (
            NotificationStream::CoreTranscript,
            NotificationDelivery::Lossless,
        ),
        AppServerNotification::TurnStarted { .. }
        | AppServerNotification::ItemStarted { .. }
        | AppServerNotification::ServerRequestRequested { .. }
        | AppServerNotification::ServerRequestResolved { .. }
        | AppServerNotification::TokenUsageUpdated { .. }
        | AppServerNotification::ModelRetrying { .. }
        | AppServerNotification::ContextCompacted { .. }
        | AppServerNotification::ContextCompactionStarted { .. }
        | AppServerNotification::TurnFailed { .. }
        | AppServerNotification::TurnCancelled { .. }
        | AppServerNotification::ConversationStatus { .. }
        | AppServerNotification::ConversationHistory { .. }
        | AppServerNotification::ConversationHistoryPage { .. }
        | AppServerNotification::ConversationList { .. }
        | AppServerNotification::ConversationSwitched { .. }
        | AppServerNotification::ConversationSubscriptionChanged { .. }
        | AppServerNotification::FrontendStateChanged { .. } => {
            (NotificationStream::Control, NotificationDelivery::Lossless)
        }
        AppServerNotification::CommandExecutionOutputDelta { .. }
        | AppServerNotification::ToolOutputDelta { .. }
        | AppServerNotification::FileChangeOutputDelta { .. } => (
            NotificationStream::Control,
            NotificationDelivery::BestEffort,
        ),
        AppServerNotification::Info { .. } | AppServerNotification::Error { .. } => (
            NotificationStream::Diagnostic,
            NotificationDelivery::BestEffort,
        ),
    }
}
