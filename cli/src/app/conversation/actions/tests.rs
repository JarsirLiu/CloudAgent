use super::prepend_turn_page;
use agent_core::{ConversationTurn, TurnState};

fn turn(id: &str) -> ConversationTurn {
    ConversationTurn {
        id: id.to_string(),
        state: TurnState::Completed,
        items: Vec::new(),
        runtime_items: Vec::new(),
        rollout_start_index: 0,
        rollout_end_index: 0,
    }
}

#[test]
fn prepend_turn_page_keeps_old_to_new_order_and_deduplicates_boundary() {
    let merged = prepend_turn_page(
        vec![turn("turn-1"), turn("turn-2")],
        vec![turn("turn-2"), turn("turn-3")],
    );
    let ids = merged.into_iter().map(|turn| turn.id).collect::<Vec<_>>();

    assert_eq!(ids, vec!["turn-1", "turn-2", "turn-3"]);
}
