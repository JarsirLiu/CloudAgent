use super::recover_inactive_running_turns;
use agent_core::{ConversationTurn, TranscriptItem, TurnState};

fn turn(id: &str, state: TurnState) -> ConversationTurn {
    ConversationTurn {
        id: id.to_string(),
        state,
        items: vec![TranscriptItem::AgentMessage {
            id: format!("assistant:{id}"),
            text: "partial".to_string(),
        }],
        rollout_start_index: 0,
        rollout_end_index: 0,
    }
}

#[test]
fn recovery_marks_orphaned_running_turn_interrupted() {
    let mut turns = vec![turn("turn-1", TurnState::Running)];

    recover_inactive_running_turns(&mut turns, None);

    assert_eq!(turns[0].state, TurnState::Cancelled);
    assert!(turns[0].items.iter().any(|item| matches!(
        item,
        TranscriptItem::SystemMessage { id, text }
            if id == "turn_interrupted:turn-1" && text.contains("Previous process exited")
    )));
}

#[test]
fn recovery_preserves_current_active_running_turn() {
    let active_turn = turn("turn-1", TurnState::Running);
    let mut turns = vec![active_turn.clone()];

    recover_inactive_running_turns(&mut turns, Some(&active_turn));

    assert_eq!(turns[0].state, TurnState::Running);
    assert!(
        !turns[0]
            .items
            .iter()
            .any(|item| item.id() == "turn_interrupted:turn-1")
    );
}
