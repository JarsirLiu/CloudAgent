use crate::conversation::TranscriptItem;
use crate::turn::{EventMsg, TurnId, TurnItemDeltaKind};

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
#[allow(clippy::large_enum_variant)]
pub enum CoreTranscriptEvent {
    TurnCompleted {
        turn_id: TurnId,
    },
    ItemCompleted {
        turn_id: TurnId,
        call_id: Option<String>,
        item: TranscriptItem,
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
    ReasoningSummaryPartAdded {
        turn_id: TurnId,
        item_id: String,
        summary_index: usize,
    },
    ReasoningSummaryTextDelta {
        turn_id: TurnId,
        item_id: String,
        summary_index: usize,
        delta: String,
    },
    ReasoningTextDelta {
        turn_id: TurnId,
        item_id: String,
        content_index: usize,
        delta: String,
    },
}

pub fn core_transcript_event_from_event_msg(event: &EventMsg) -> Option<CoreTranscriptEvent> {
    match event {
        EventMsg::ItemDelta {
            turn_id,
            item_id,
            kind,
            segment_index,
            delta,
            ..
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
                    summary_index: segment_index.unwrap_or(0),
                    delta: delta.clone(),
                })
            }
            TurnItemDeltaKind::ReasoningText => Some(CoreTranscriptEvent::ReasoningTextDelta {
                turn_id: turn_id.clone(),
                item_id: item_id.clone(),
                content_index: segment_index.unwrap_or(0),
                delta: delta.clone(),
            }),
            TurnItemDeltaKind::CommandExecutionOutput
            | TurnItemDeltaKind::ToolOutput
            | TurnItemDeltaKind::FileChangeOutput
            | TurnItemDeltaKind::JsonPatch => None,
        },
        EventMsg::ItemCompleted {
            turn_id,
            call_id,
            item,
            ..
        } => Some(CoreTranscriptEvent::ItemCompleted {
            turn_id: turn_id.clone(),
            call_id: call_id.clone(),
            item: item.clone(),
        }),
        EventMsg::TurnCompleted { turn_id, .. } => Some(CoreTranscriptEvent::TurnCompleted {
            turn_id: turn_id.clone(),
        }),
        EventMsg::TurnStarted { .. }
        | EventMsg::ModelRequestStarted { .. }
        | EventMsg::ModelResponseReceived { .. }
        | EventMsg::TokenUsageUpdated { .. }
        | EventMsg::ContextCompacted { .. }
        | EventMsg::ContextCompactionStarted { .. }
        | EventMsg::ModelRetrying { .. }
        | EventMsg::ItemStarted { .. }
        | EventMsg::ServerRequestRequested { .. }
        | EventMsg::ServerRequestResolved { .. }
        | EventMsg::TurnFailed { .. }
        | EventMsg::TurnCancelled { .. } => None,
    }
}

pub fn classify_event_msg(event: &EventMsg) -> (EventStream, EventDelivery) {
    match event {
        EventMsg::ItemDelta { kind, .. } => match kind {
            TurnItemDeltaKind::Text
            | TurnItemDeltaKind::ReasoningSummary
            | TurnItemDeltaKind::ReasoningText => {
                (EventStream::CoreTranscript, EventDelivery::Lossless)
            }
            TurnItemDeltaKind::CommandExecutionOutput
            | TurnItemDeltaKind::ToolOutput
            | TurnItemDeltaKind::FileChangeOutput => {
                (EventStream::Control, EventDelivery::BestEffort)
            }
            TurnItemDeltaKind::JsonPatch => (EventStream::Diagnostic, EventDelivery::InternalOnly),
        },
        EventMsg::ItemCompleted { .. } | EventMsg::TurnCompleted { .. } => {
            (EventStream::CoreTranscript, EventDelivery::Lossless)
        }
        EventMsg::ItemStarted { .. }
        | EventMsg::ServerRequestRequested { .. }
        | EventMsg::ServerRequestResolved { .. }
        | EventMsg::TurnStarted { .. }
        | EventMsg::TurnFailed { .. }
        | EventMsg::TurnCancelled { .. } => (EventStream::Control, EventDelivery::Lossless),
        EventMsg::ModelRequestStarted { .. }
        | EventMsg::ModelResponseReceived { .. }
        | EventMsg::TokenUsageUpdated { .. }
        | EventMsg::ModelRetrying { .. }
        | EventMsg::ContextCompacted { .. } => (EventStream::Diagnostic, EventDelivery::BestEffort),
        EventMsg::ContextCompactionStarted { .. } => {
            (EventStream::Diagnostic, EventDelivery::BestEffort)
        }
    }
}
