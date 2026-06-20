use super::AgentState;

#[tokio::test]
async fn same_conversation_keeps_single_active_turn_slot() {
    let state = AgentState::new("system".to_string());
    let first = state
        .start_turn("conv-a".to_string(), "turn-1".to_string())
        .await;
    let second = state
        .start_turn("conv-a".to_string(), "turn-2".to_string())
        .await;

    let active = state.active_turn("conv-a").await.expect("active turn");
    assert_eq!(active.turn_id, "turn-1");
    assert!(!active.is_cancelled());
    assert_eq!(first.expect("first accepted").turn_id, "turn-1");
    assert!(second.is_none());
}

#[tokio::test]
async fn same_conversation_second_start_is_rejected_when_busy() {
    let state = AgentState::new("system".to_string());
    state
        .start_turn("conv-a".to_string(), "turn-1".to_string())
        .await;
    let second = state
        .start_turn("conv-a".to_string(), "turn-2".to_string())
        .await;

    let active = state.active_turn("conv-a").await.expect("active turn");
    assert_eq!(active.turn_id, "turn-1");
    assert!(second.is_none());
}

#[tokio::test]
async fn different_conversations_can_run_in_parallel() {
    let state = AgentState::new("system".to_string());
    state
        .start_turn("conv-a".to_string(), "turn-a".to_string())
        .await;
    state
        .start_turn("conv-b".to_string(), "turn-b".to_string())
        .await;

    let active_a = state.active_turn("conv-a").await.expect("active a");
    let active_b = state.active_turn("conv-b").await.expect("active b");
    assert_eq!(active_a.turn_id, "turn-a");
    assert_eq!(active_b.turn_id, "turn-b");
    assert!(!active_a.is_cancelled());
    assert!(!active_b.is_cancelled());
}
