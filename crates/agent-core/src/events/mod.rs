use crate::conversation::ThreadItem;
use crate::turn::{TurnEvent, TurnId, TurnItemDeltaKind};

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

#[derive(Clone, Debug)]
pub enum CoreTranscriptEvent {
    TurnCompleted {
        turn_id: TurnId,
    },
    ItemCompleted {
        turn_id: TurnId,
        item: ThreadItem,
    },
    AgentMessageDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    PlanDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningSummaryTextDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningTextDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
}

pub fn core_transcript_event_from_turn_event(event: &TurnEvent) -> Option<CoreTranscriptEvent> {
    match event {
        TurnEvent::ItemDelta {
            turn_id,
            item_id,
            kind,
            delta,
        } => match kind {
            TurnItemDeltaKind::Text => Some(CoreTranscriptEvent::AgentMessageDelta {
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                delta: delta.clone(),
            }),
            TurnItemDeltaKind::ReasoningSummary => {
                Some(CoreTranscriptEvent::ReasoningSummaryTextDelta {
                    turn_id: turn_id.clone(),
                    item_id: item_id.clone(),
                    delta: delta.clone(),
                })
            }
            TurnItemDeltaKind::ReasoningText => Some(CoreTranscriptEvent::ReasoningTextDelta {
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                delta: delta.clone(),
            }),
            TurnItemDeltaKind::ToolOutput | TurnItemDeltaKind::JsonPatch => None,
        },
        TurnEvent::ItemCompleted { turn_id, item, .. } => {
            Some(CoreTranscriptEvent::ItemCompleted {
                turn_id: turn_id.clone(),
                item: item.clone(),
            })
        }
        TurnEvent::TurnCompleted { turn_id, .. } => Some(CoreTranscriptEvent::TurnCompleted {
            turn_id: turn_id.clone(),
        }),
        TurnEvent::TurnStarted { .. }
        | TurnEvent::ModelRequestStarted { .. }
        | TurnEvent::ModelResponseReceived { .. }
        | TurnEvent::ItemStarted { .. }
        | TurnEvent::ServerRequestRequested { .. }
        | TurnEvent::ServerRequestResolved { .. }
        | TurnEvent::TurnFailed { .. }
        | TurnEvent::TurnCancelled { .. } => None,
    }
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
