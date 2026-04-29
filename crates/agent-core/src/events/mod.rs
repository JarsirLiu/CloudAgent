use crate::turn::{TurnEvent, TurnItemDeltaKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDelivery {
    Lossless,
    BestEffort,
    InternalOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventStream {
    CoreTranscript,
    Control,
    Diagnostic,
}

pub fn classify_turn_event(event: &TurnEvent) -> (EventStream, EventDelivery) {
    match event {
        TurnEvent::ItemDelta { kind, .. } => match kind {
            TurnItemDeltaKind::Text
            | TurnItemDeltaKind::ReasoningSummary
            | TurnItemDeltaKind::ReasoningText => {
                (EventStream::CoreTranscript, EventDelivery::Lossless)
            }
            TurnItemDeltaKind::ToolOutput => (EventStream::Control, EventDelivery::BestEffort),
            TurnItemDeltaKind::JsonPatch => (EventStream::Diagnostic, EventDelivery::InternalOnly),
        },
        TurnEvent::ItemCompleted { .. } | TurnEvent::TurnCompleted { .. } => {
            (EventStream::CoreTranscript, EventDelivery::Lossless)
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

pub fn module_name() -> &'static str {
    "agent-core::events"
}
