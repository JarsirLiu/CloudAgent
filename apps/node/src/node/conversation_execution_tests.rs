use super::ConversationExecutionRegistry;
use agent_protocol::ConversationViewStatus;

#[test]
fn conversation_view_updates_busy_state() {
    let mut registry = ConversationExecutionRegistry::default();

    registry.update_conversation_view(
        "conversation-1",
        &ConversationViewStatus::Active {
            active_turn_id: Some("turn-1".to_string()),
            flags: Vec::new(),
        },
    );
    assert!(registry.is_busy("conversation-1"));

    registry.update_conversation_view("conversation-1", &ConversationViewStatus::Idle);
    assert!(!registry.is_busy("conversation-1"));
}
