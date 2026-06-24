use super::reducer_messages::{
    context_compacted_message, preview_excerpt, server_request_resolved_message,
};
use agent_core::ServerRequestDecision;

#[test]
fn preview_excerpt_trims_and_truncates_long_arguments() {
    assert_eq!(preview_excerpt("  "), "(none)");
    assert_eq!(preview_excerpt("hello"), "hello");
    assert_eq!(
        preview_excerpt(&"a".repeat(81)),
        format!("{}… (truncated)", "a".repeat(80))
    );
}

#[test]
fn message_builders_keep_notice_copy_stable() {
    assert_eq!(
        context_compacted_message(120, 45),
        "Context compacted: ~120 -> ~45 tokens"
    );
    assert_eq!(
        server_request_resolved_message(&ServerRequestDecision::accept(Some("ok".to_string()))),
        "Request approved: ok"
    );
}
