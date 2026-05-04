use crate::{ConversationTurn, EventMsg};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn next_turn_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("turn-{now}")
}

pub fn emit_event<E>(events: &mut Vec<EventMsg>, on_event: &mut E, event: EventMsg)
where
    E: FnMut(&EventMsg) + ?Sized,
{
    events.push(event.clone());
    on_event(&event);
}

pub fn paginate_turns(
    turns: Vec<ConversationTurn>,
    before_turn_id: Option<&str>,
    limit: usize,
) -> (Vec<ConversationTurn>, bool, Option<String>) {
    if turns.is_empty() {
        return (Vec::new(), false, None);
    }
    let end_exclusive = if let Some(before_id) = before_turn_id {
        turns
            .iter()
            .position(|turn| turn.id == before_id)
            .unwrap_or(turns.len())
    } else {
        turns.len()
    };
    let page_limit = limit.max(1);
    let start = end_exclusive.saturating_sub(page_limit);
    let page = turns[start..end_exclusive].to_vec();
    let has_more = start > 0;
    let next_before_turn_id = if has_more {
        Some(turns[start].id.clone())
    } else {
        None
    };
    (page, has_more, next_before_turn_id)
}
