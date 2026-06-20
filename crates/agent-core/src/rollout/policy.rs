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
#[path = "policy_tests.rs"]
mod tests;
