use super::*;
use crate::conversation::TranscriptItem;
use crate::model::ModelUsage;
use crate::turn::{TurnItemDeltaKind, TurnItemKind};

fn event_item(event: EventMsg) -> RolloutItem {
    RolloutItem::from(event)
}

#[test]
fn filters_streaming_lifecycle_events() {
    let items = vec![
        event_item(EventMsg::ItemStarted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemKind::AssistantMessage,
            title: None,
        }),
        event_item(EventMsg::ItemDelta {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            kind: TurnItemDeltaKind::Text,
            segment_index: None,
            delta: "hello".to_string(),
        }),
        event_item(EventMsg::ItemCompleted {
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            call_id: None,
            item: TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "hello".to_string(),
            },
        }),
    ];

    let persisted = persisted_rollout_items(&items, RolloutPersistenceMode::Limited);

    assert!(persisted.is_empty());
}

#[test]
fn keeps_final_response_items_and_turn_state() {
    let items = vec![
        event_item(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: "conv".to_string(),
            user_input: Vec::new(),
        }),
        RolloutItem::from(ResponseItem::Assistant {
            content: Some("done".to_string()),
            reasoning: Some("thinking".to_string()),
            tool_calls: Vec::new(),
        }),
        event_item(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }),
    ];

    let persisted = persisted_rollout_items(&items, RolloutPersistenceMode::Limited);

    assert_eq!(persisted.len(), 3);
}

#[test]
fn keeps_token_usage_updates() {
    let usage = ModelUsage {
        input_tokens: 1,
        output_tokens: 2,
        reasoning_output_tokens: 0,
        total_tokens: 3,
        cached_input_tokens: 0,
    };
    let items = vec![event_item(EventMsg::TokenUsageUpdated {
        turn_id: "turn-1".to_string(),
        last_usage: usage.clone(),
        total_usage: usage,
        model_context_window: Some(128_000),
        request_estimated_tokens: 42,
    })];

    let persisted = persisted_rollout_items(&items, RolloutPersistenceMode::Limited);

    assert_eq!(persisted.len(), 1);
}
