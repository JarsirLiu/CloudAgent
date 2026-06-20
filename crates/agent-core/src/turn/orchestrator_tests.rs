use super::*;
use crate::ConversationHistory;

#[test]
fn ensure_terminal_event_appends_completed_when_missing() {
    let mut outcome = TurnOutcome {
        turn_id: "turn-1".to_string(),
        events: Vec::new(),
        history: ConversationHistory::new("conv-1".to_string(), "system".to_string()),
        model_name: None,
        state: TurnState::Completed,
    };
    let mut delivered = Vec::new();
    ensure_terminal_event(&mut outcome, "turn-1", &mut |event| {
        delivered.push(event.clone());
    });

    assert!(matches!(
        outcome.events.as_slice(),
        [EventMsg::TurnCompleted { turn_id }] if turn_id == "turn-1"
    ));
    assert!(matches!(
        delivered.as_slice(),
        [EventMsg::TurnCompleted { turn_id }] if turn_id == "turn-1"
    ));
}
