pub const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TruncatedText {
    pub text: String,
    pub original_token_count: usize,
    pub truncated: bool,
}

pub fn approximate_token_count(text: &str) -> usize {
    text.chars().count().saturating_add(2) / 3
}

pub fn truncate_text_to_token_budget(
    text: &str,
    max_tokens: usize,
    truncation_notice: Option<&str>,
) -> TruncatedText {
    let max_tokens = max_tokens.max(1);
    let original_token_count = approximate_token_count(text);
    if original_token_count <= max_tokens {
        return TruncatedText {
            text: text.to_string(),
            original_token_count,
            truncated: false,
        };
    }

    let marker = format!(
        "\n[{} tokens truncated]\n",
        original_token_count - max_tokens
    );
    let notice = truncation_notice.unwrap_or_default();
    let marker_tokens = approximate_token_count(&marker).max(1);
    let notice_tokens = approximate_token_count(notice);
    let content_budget = max_tokens
        .saturating_sub(marker_tokens)
        .saturating_sub(notice_tokens)
        .max(1);
    let head_tokens = content_budget * 2 / 3;
    let tail_tokens = content_budget.saturating_sub(head_tokens);
    let head_chars = head_tokens.saturating_mul(3).max(1);
    let tail_chars = tail_tokens.saturating_mul(3).max(1);
    let head = text.chars().take(head_chars).collect::<String>();
    let tail = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();

    TruncatedText {
        text: format!("{head}{marker}{tail}{notice}"),
        original_token_count,
        truncated: true,
    }
}

#[cfg(test)]
mod tests {
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
}
