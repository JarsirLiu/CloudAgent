use agent_protocol::{AppServerNotification, TurnEvent, TurnItemDeltaKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDelivery {
    Lossless,
    BestEffort,
    InternalOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventStream {
    Transcript,
    Control,
    Diagnostic,
}

pub fn classify_turn_event(event: &TurnEvent) -> (EventStream, EventDelivery) {
    match event {
        TurnEvent::ItemDelta { kind, .. } => match kind {
            TurnItemDeltaKind::Text
            | TurnItemDeltaKind::ReasoningSummary
            | TurnItemDeltaKind::ReasoningText => (EventStream::Transcript, EventDelivery::Lossless),
            TurnItemDeltaKind::ToolOutput => (EventStream::Control, EventDelivery::BestEffort),
            TurnItemDeltaKind::JsonPatch => (EventStream::Diagnostic, EventDelivery::InternalOnly),
        },
        TurnEvent::ItemCompleted { .. } | TurnEvent::TurnCompleted { .. } => {
            (EventStream::Transcript, EventDelivery::Lossless)
        }
        TurnEvent::ItemStarted { .. }
        | TurnEvent::ServerRequestRequested { .. }
        | TurnEvent::ServerRequestResolved { .. }
        | TurnEvent::TurnStarted { .. }
        | TurnEvent::TurnFailed { .. }
        | TurnEvent::TurnCancelled { .. } => (EventStream::Control, EventDelivery::Lossless),
        TurnEvent::ModelRequestStarted { .. } | TurnEvent::ModelResponseReceived { .. } => {
            (EventStream::Diagnostic, EventDelivery::BestEffort)
        }
    }
}

pub fn classify_notification(notification: &AppServerNotification) -> (EventStream, EventDelivery) {
    match notification {
        AppServerNotification::ItemDelta { kind, .. } => match kind {
            TurnItemDeltaKind::Text
            | TurnItemDeltaKind::ReasoningSummary
            | TurnItemDeltaKind::ReasoningText => (EventStream::Transcript, EventDelivery::Lossless),
            TurnItemDeltaKind::ToolOutput => (EventStream::Control, EventDelivery::BestEffort),
            TurnItemDeltaKind::JsonPatch => (EventStream::Diagnostic, EventDelivery::InternalOnly),
        },
        AppServerNotification::ItemCompleted { .. } | AppServerNotification::TurnCompleted { .. } => {
            (EventStream::Transcript, EventDelivery::Lossless)
        }
        AppServerNotification::ItemStarted { .. }
        | AppServerNotification::ServerRequestRequested { .. }
        | AppServerNotification::ServerRequestResolved { .. }
        | AppServerNotification::TurnStarted { .. }
        | AppServerNotification::TurnFailed { .. }
        | AppServerNotification::TurnCancelled { .. } => {
            (EventStream::Control, EventDelivery::Lossless)
        }
        AppServerNotification::FrontendStateChanged { .. }
        | AppServerNotification::ConversationStatus { .. }
        | AppServerNotification::ConversationHistory { .. }
        | AppServerNotification::ConversationSubscriptionChanged { .. }
        | AppServerNotification::Info { .. }
        | AppServerNotification::Error { .. } => (EventStream::Diagnostic, EventDelivery::BestEffort),
    }
}

pub fn module_name() -> &'static str {
    "agent-core::events"
}
