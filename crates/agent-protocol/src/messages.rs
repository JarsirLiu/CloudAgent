use crate::types::{SessionSnapshot, FrontendMode, NotificationDelivery, NotificationStream};
use crate::{
    SessionSummary, ConversationTurn, ModelUsage, RequestId, ServerRequest,
    ServerRequestDecision, TranscriptItem, TurnId, TurnItemKind, UserTurnInput,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppClientCommand {
    SubmitTurn(UserTurnInput),
    ResolveServerRequest {
        session_id: String,
        request_id: RequestId,
        decision: ServerRequestDecision,
    },
    InterruptTurn {
        session_id: String,
    },
    CompactSession {
        session_id: String,
    },
    ResetSession {
        session_id: String,
    },
    RequestSessionStatus {
        session_id: String,
    },
    RequestSessionHistory {
        session_id: String,
    },
    ListSessions,
    CreateSession {
        session_id: String,
    },
    SwitchSession {
        session_id: String,
    },
    ArchiveSession {
        session_id: String,
    },
    SubscribeSession {
        session_id: String,
    },
    UnsubscribeSession {
        session_id: String,
    },
    Exit,
}

impl AppClientCommand {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::SubmitTurn(input) => Some(&input.session_id),
            Self::ResolveServerRequest {
                session_id, ..
            }
            | Self::InterruptTurn { session_id }
            | Self::CompactSession { session_id }
            | Self::ResetSession { session_id }
            | Self::RequestSessionStatus { session_id }
            | Self::RequestSessionHistory { session_id }
            | Self::CreateSession { session_id }
            | Self::SwitchSession { session_id }
            | Self::ArchiveSession { session_id }
            | Self::SubscribeSession { session_id }
            | Self::UnsubscribeSession { session_id } => Some(session_id),
            Self::ListSessions | Self::Exit => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerNotification {
    FrontendStateChanged {
        session_id: String,
        mode: FrontendMode,
    },
    TurnStarted {
        session_id: String,
        turn_id: TurnId,
    },
    ItemStarted {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
    },
    AgentMessageDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    PlanDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningSummaryTextDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningTextDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    CommandExecutionOutputDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ToolOutputDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    FileChangeOutputDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    TokenUsageUpdated {
        session_id: String,
        turn_id: TurnId,
        last_usage: ModelUsage,
        total_usage: ModelUsage,
        model_context_window: Option<u64>,
    },
    ContextCompacted {
        session_id: String,
        turn_id: TurnId,
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    ContextCompactionStarted {
        session_id: String,
        turn_id: TurnId,
        estimated_tokens: u64,
    },
    ItemCompleted {
        session_id: String,
        turn_id: TurnId,
        item: TranscriptItem,
    },
    ServerRequestRequested {
        session_id: String,
        turn_id: TurnId,
        request: ServerRequest,
    },
    ServerRequestResolved {
        session_id: String,
        turn_id: TurnId,
        request_id: RequestId,
        request: ServerRequest,
        decision: ServerRequestDecision,
    },
    TurnCompleted {
        session_id: String,
        turn_id: TurnId,
    },
    TurnFailed {
        session_id: String,
        turn_id: TurnId,
        error: String,
    },
    TurnCancelled {
        session_id: String,
        turn_id: TurnId,
        reason: String,
    },
    SessionStatus {
        session_id: String,
        snapshot: SessionSnapshot,
    },
    SessionHistory {
        session_id: String,
        turns: Vec<ConversationTurn>,
    },
    SessionList {
        session_id: String,
        conversations: Vec<SessionSummary>,
    },
    SessionSwitched {
        session_id: String,
    },
    SessionSubscriptionChanged {
        session_id: String,
        subscribed: bool,
    },
    Info {
        session_id: String,
        message: String,
    },
    Error {
        session_id: String,
        message: String,
    },
}

impl AppServerNotification {
    pub fn session_id(&self) -> &str {
        match self {
            Self::FrontendStateChanged {
                session_id, ..
            }
            | Self::TurnStarted {
                session_id, ..
            }
            | Self::ItemStarted {
                session_id, ..
            }
            | Self::AgentMessageDelta {
                session_id, ..
            }
            | Self::PlanDelta {
                session_id, ..
            }
            | Self::ReasoningSummaryTextDelta {
                session_id, ..
            }
            | Self::ReasoningTextDelta {
                session_id, ..
            }
            | Self::CommandExecutionOutputDelta {
                session_id, ..
            }
            | Self::ToolOutputDelta {
                session_id, ..
            }
            | Self::FileChangeOutputDelta {
                session_id, ..
            }
            | Self::TokenUsageUpdated {
                session_id, ..
            }
            | Self::ContextCompacted {
                session_id, ..
            }
            | Self::ContextCompactionStarted {
                session_id, ..
            }
            | Self::ItemCompleted {
                session_id, ..
            }
            | Self::ServerRequestRequested {
                session_id, ..
            }
            | Self::ServerRequestResolved {
                session_id, ..
            }
            | Self::TurnCompleted {
                session_id, ..
            }
            | Self::TurnFailed {
                session_id, ..
            }
            | Self::TurnCancelled {
                session_id, ..
            }
            | Self::SessionStatus {
                session_id, ..
            }
            | Self::SessionHistory {
                session_id, ..
            }
            | Self::SessionList {
                session_id, ..
            }
            | Self::SessionSwitched {
                session_id, ..
            }
            | Self::SessionSubscriptionChanged {
                session_id, ..
            }
            | Self::Info {
                session_id, ..
            }
            | Self::Error {
                session_id, ..
            } => session_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerRequest {
    ServerRequest {
        request_id: RequestId,
        session_id: String,
        request: ServerRequest,
    },
}

impl AppServerRequest {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::ServerRequest { request_id, .. } => request_id,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::ServerRequest {
                session_id, ..
            } => session_id,
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
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Notification(notification) => Some(notification.session_id()),
            Self::Request(request) => Some(request.session_id()),
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
        | AppServerNotification::ContextCompacted { .. }
        | AppServerNotification::ContextCompactionStarted { .. }
        | AppServerNotification::TurnFailed { .. }
        | AppServerNotification::TurnCancelled { .. }
        | AppServerNotification::SessionStatus { .. }
        | AppServerNotification::SessionHistory { .. }
        | AppServerNotification::SessionList { .. }
        | AppServerNotification::SessionSwitched { .. }
        | AppServerNotification::SessionSubscriptionChanged { .. }
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

