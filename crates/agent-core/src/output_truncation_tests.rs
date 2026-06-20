use super::*;

#[test]
fn truncate_text_to_token_budget_keeps_head_tail_and_count() {
    let text = format!("{}middle{}", "a".repeat(12_000), "z".repeat(12_000));
    let truncated = truncate_text_to_token_budget(&text, 1_000, Some("\n[narrow output]\n"));

    assert!(truncated.truncated);
    assert!(truncated.original_token_count > 1_000);
    assert!(truncated.text.contains("tokens truncated"));
    assert!(truncated.text.contains("narrow output"));
    assert!(truncated.text.starts_with('a'));
    assert!(truncated.text.contains('z'));
    assert!(truncated.text.len() < text.len());
}

#[test]
fn truncate_text_to_token_budget_leaves_small_text_unchanged() {
    let truncated = truncate_text_to_token_budget("hello", 1_000, None);

    assert!(!truncated.truncated);
    assert_eq!(truncated.text, "hello");
}
