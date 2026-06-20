use super::conversation_busy_error;

#[test]
fn conversation_busy_error_is_stable() {
    assert_eq!(
        conversation_busy_error(),
        "ERR_CONVERSATION_BUSY: conversation is busy; concurrent turns on the same conversation are not allowed"
    );
}
