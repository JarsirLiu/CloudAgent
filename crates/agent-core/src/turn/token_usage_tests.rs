use super::*;
use crate::turn::CompactionContinuation;

fn usage(total: u64) -> ModelUsage {
    ModelUsage {
        input_tokens: total,
        total_tokens: total,
        ..ModelUsage::default()
    }
}

#[test]
fn restores_latest_server_usage_as_session_total() {
    let items = vec![
        RolloutItem::from(EventMsg::TokenUsageUpdated {
            turn_id: "turn-1".to_string(),
            last_usage: usage(120),
            total_usage: usage(120),
            model_context_window: Some(200_000),
            request_estimated_tokens: 110,
        }),
        RolloutItem::from(EventMsg::TokenUsageUpdated {
            turn_id: "turn-2".to_string(),
            last_usage: usage(240),
            total_usage: usage(360),
            model_context_window: Some(200_000),
            request_estimated_tokens: 222,
        }),
    ];

    let restored =
        latest_turn_token_state_from_rollout_items(&items).expect("restored usage baseline");

    assert_eq!(restored.request_baseline.server_context_tokens, Some(240));
    assert_eq!(
        restored.request_baseline.request_estimated_tokens,
        Some(222)
    );
    assert_eq!(restored.usage.total_usage.total_tokens, 360);
    assert_eq!(
        restored.usage.active_context_tokens_from_last_usage(),
        Some(240)
    );
}

#[test]
fn compaction_resets_window_baseline_without_discarding_session_usage() {
    let items = vec![
        RolloutItem::from(EventMsg::TokenUsageUpdated {
            turn_id: "turn-1".to_string(),
            last_usage: usage(185_000),
            total_usage: usage(185_000),
            model_context_window: Some(200_000),
            request_estimated_tokens: 180_000,
        }),
        RolloutItem::from(EventMsg::ContextCompacted {
            turn_id: "turn-2".to_string(),
            continuation: CompactionContinuation::PreTurn,
            pre_context_tokens_estimate: 190_000,
            post_context_tokens_estimate: 32_000,
            pre_message_count: 40,
            post_message_count: 6,
            preserved_user_count: 3,
        }),
    ];

    let restored =
        latest_turn_token_state_from_rollout_items(&items).expect("restored compacted baseline");

    assert_eq!(
        restored.request_baseline.server_context_tokens,
        Some(32_000)
    );
    assert_eq!(
        restored.request_baseline.request_estimated_tokens,
        Some(32_000)
    );
    assert_eq!(restored.usage.total_usage.total_tokens, 185_000);
    assert_eq!(
        restored.auto_compact_window.prefill_input_tokens,
        Some(32_000)
    );
}

#[test]
fn server_usage_appends_to_session_total() {
    let mut state = TokenUsageState::default();

    state.append_server_usage(usage(120), Some(200_000));
    state.append_server_usage(usage(240), Some(200_000));

    assert_eq!(state.total_usage.total_tokens, 360);
    assert_eq!(state.active_context_tokens_from_last_usage(), Some(240));
}

#[test]
fn signed_delta_handles_smaller_next_request() {
    assert_eq!(apply_signed_token_delta(32_000, 28_000, 32_000), 28_000);
    assert_eq!(apply_signed_token_delta(32_000, 40_000, 32_000), 40_000);
}
