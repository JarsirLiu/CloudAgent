use super::auto_compact_policy::{
    AutoCompactPolicyInput, AutoCompactTokenLimitScope, auto_compact_token_status,
    effective_auto_compact_limit,
};
use super::auto_compact_window::AutoCompactWindowSnapshot;

fn snapshot(prefill_input_tokens: Option<usize>) -> AutoCompactWindowSnapshot {
    AutoCompactWindowSnapshot {
        ordinal: 2,
        prefill_input_tokens,
    }
}

#[test]
fn default_limit_is_context_window_times_ratio() {
    assert_eq!(effective_auto_compact_limit(200_000, 0.9, None), 180_000);
}

#[test]
fn configured_limit_is_capped_by_ratio_limit() {
    assert_eq!(
        effective_auto_compact_limit(200_000, 0.9, Some(120_000)),
        120_000
    );
    assert_eq!(
        effective_auto_compact_limit(200_000, 0.9, Some(190_000)),
        180_000
    );
}

#[test]
fn total_scope_counts_full_active_context() {
    let status = auto_compact_token_status(AutoCompactPolicyInput {
        model_context_window: 200_000,
        trigger_ratio: 0.9,
        configured_limit: None,
        scope: AutoCompactTokenLimitScope::Total,
        active_context_tokens: 180_000,
        window: snapshot(Some(50_000)),
    });

    assert_eq!(status.scope_tokens, 180_000);
    assert!(status.token_limit_reached);
    assert_eq!(status.window_prefill_tokens, None);
}

#[test]
fn body_after_prefix_subtracts_prefill_baseline() {
    let status = auto_compact_token_status(AutoCompactPolicyInput {
        model_context_window: 200_000,
        trigger_ratio: 0.9,
        configured_limit: None,
        scope: AutoCompactTokenLimitScope::BodyAfterPrefix,
        active_context_tokens: 190_000,
        window: snapshot(Some(50_000)),
    });

    assert_eq!(status.scope_tokens, 140_000);
    assert_eq!(status.limit_tokens, 180_000);
    assert!(!status.token_limit_reached);
    assert_eq!(status.window_prefill_tokens, Some(50_000));
}

#[test]
fn body_after_prefix_without_prefill_uses_active_context_as_baseline() {
    let status = auto_compact_token_status(AutoCompactPolicyInput {
        model_context_window: 200_000,
        trigger_ratio: 0.9,
        configured_limit: None,
        scope: AutoCompactTokenLimitScope::BodyAfterPrefix,
        active_context_tokens: 190_000,
        window: snapshot(None),
    });

    assert_eq!(status.scope_tokens, 0);
    assert!(!status.token_limit_reached);
}

#[test]
fn body_after_prefix_still_triggers_at_full_context_window() {
    let status = auto_compact_token_status(AutoCompactPolicyInput {
        model_context_window: 200_000,
        trigger_ratio: 0.9,
        configured_limit: None,
        scope: AutoCompactTokenLimitScope::BodyAfterPrefix,
        active_context_tokens: 200_000,
        window: snapshot(Some(50_000)),
    });

    assert!(status.full_context_window_limit_reached);
    assert!(status.token_limit_reached);
}
