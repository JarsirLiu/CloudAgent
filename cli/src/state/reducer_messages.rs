use agent_core::ServerRequestDecision;

pub(crate) fn server_request_resolved_message(decision: &ServerRequestDecision) -> String {
    format!(
        "Request {}{}",
        decision.label(),
        decision
            .reason
            .as_deref()
            .map(|r| format!(": {r}"))
            .unwrap_or_default()
    )
}

pub(crate) fn context_compacted_message(
    pre_context_tokens_estimate: u64,
    post_context_tokens_estimate: u64,
) -> String {
    format!(
        "Context compacted: ~{} -> ~{} tokens",
        pre_context_tokens_estimate, post_context_tokens_estimate
    )
}

pub(crate) fn preview_excerpt(arguments_preview: &str) -> String {
    let trimmed = arguments_preview.trim();
    if trimmed.is_empty() {
        return "(none)".to_string();
    }
    if trimmed.chars().count() <= 80 {
        return trimmed.to_string();
    }
    let mut out = String::new();
    for ch in trimmed.chars().take(80) {
        out.push(ch);
    }
    out.push_str("… (truncated)");
    out
}
