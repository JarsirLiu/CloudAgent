use crate::conversation::ResponseItem;
use crate::rollout::RolloutItem;
use crate::turn::EventMsg;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RolloutPersistenceMode {
    #[default]
    Limited,
}

pub fn persisted_rollout_items(
    items: &[RolloutItem],
    mode: RolloutPersistenceMode,
) -> Vec<RolloutItem> {
    items
        .iter()
        .filter_map(|item| persisted_rollout_item(item, mode))
        .collect()
}

pub fn persisted_rollout_item(
    item: &RolloutItem,
    mode: RolloutPersistenceMode,
) -> Option<RolloutItem> {
    match item {
        RolloutItem::EventMsg { event } => {
            should_persist_event_msg(event, mode).then(|| item.clone())
        }
        RolloutItem::ResponseItem {
            item: response_item,
        } => should_persist_response_item(response_item).then(|| item.clone()),
        RolloutItem::Compacted { .. } => Some(item.clone()),
    }
}

fn should_persist_response_item(item: &ResponseItem) -> bool {
    match item {
        ResponseItem::System { .. }
        | ResponseItem::User { .. }
        | ResponseItem::Assistant { .. }
        | ResponseItem::Tool { .. } => true,
    }
}

fn should_persist_event_msg(event: &EventMsg, mode: RolloutPersistenceMode) -> bool {
    match mode {
        RolloutPersistenceMode::Limited => should_persist_event_msg_limited(event),
    }
}

fn should_persist_event_msg_limited(event: &EventMsg) -> bool {
    match event {
        EventMsg::TurnStarted { .. }
        | EventMsg::TokenUsageUpdated { .. }
        | EventMsg::ContextCompacted { .. }
        | EventMsg::ServerRequestRequested { .. }
        | EventMsg::ServerRequestResolved { .. }
        | EventMsg::TurnCompleted { .. }
        | EventMsg::TurnFailed { .. }
        | EventMsg::TurnCancelled { .. } => true,

        EventMsg::ModelRequestStarted { .. }
        | EventMsg::ModelResponseReceived { .. }
        | EventMsg::ModelRetrying { .. }
        | EventMsg::ContextCompactionStarted { .. }
        | EventMsg::ItemStarted { .. }
        | EventMsg::ItemDelta { .. }
        | EventMsg::ItemCompleted { .. } => false,
    }
}

#[cfg(test)]
mod tests {
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
}
