pub const CONVERSATION_BUSY_ERROR_CODE: &str = "ERR_CONVERSATION_BUSY";
pub const CONVERSATION_BUSY_ERROR_MESSAGE: &str =
    "conversation is busy; concurrent turns on the same conversation are not allowed";

pub fn conversation_busy_error() -> String {
    format!(
        "{}: {}",
        CONVERSATION_BUSY_ERROR_CODE, CONVERSATION_BUSY_ERROR_MESSAGE
    )
}

#[cfg(test)]
mod tests {
    use super::conversation_busy_error;

    #[test]
    fn conversation_busy_error_is_stable() {
        assert_eq!(
            conversation_busy_error(),
            "ERR_CONVERSATION_BUSY: conversation is busy; concurrent turns on the same conversation are not allowed"
        );
    }
}
