use crate::conversation::{ConversationHistory, TranscriptItem};
use crate::turn::{EventMsg, TurnItemDeltaKind, TurnItemKind, TurnState, emit_event};

#[derive(Clone, Debug)]
pub struct TurnOutcome {
    pub turn_id: String,
    pub events: Vec<EventMsg>,
    pub history: ConversationHistory,
    pub model_name: Option<String>,
    pub state: TurnState,
}

pub fn emit_assistant_message_item(
    events: &mut Vec<EventMsg>,
    on_event: &mut (impl FnMut(&EventMsg) + ?Sized),
    turn_id: &str,
    content: &str,
    assistant_item_seq: &mut usize,
) {
    let assistant_item_id = format!("assistant:{turn_id}:{}", *assistant_item_seq);
    *assistant_item_seq += 1;
    emit_event(
        events,
        on_event,
        EventMsg::ItemStarted {
            turn_id: turn_id.to_string(),
            item_id: assistant_item_id.clone(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: Some("assistant_message".to_string()),
        },
    );
    emit_event(
        events,
        on_event,
        EventMsg::ItemDelta {
            turn_id: turn_id.to_string(),
            item_id: assistant_item_id.clone(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: content.to_string(),
        },
    );
    emit_event(
        events,
        on_event,
        EventMsg::ItemCompleted {
            turn_id: turn_id.to_string(),
            item_id: assistant_item_id.clone(),
            call_id: None,
            item: TranscriptItem::AgentMessage {
                id: assistant_item_id,
                text: content.to_string(),
            },
        },
    );
}
