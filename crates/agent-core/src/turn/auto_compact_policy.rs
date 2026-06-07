use super::auto_compact_window::AutoCompactWindowSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoCompactTokenLimitScope {
    #[default]
    Total,
    BodyAfterPrefix,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoCompactPolicyInput {
    pub model_context_window: usize,
    pub trigger_ratio: f32,
    pub configured_limit: Option<usize>,
    pub scope: AutoCompactTokenLimitScope,
    pub active_context_tokens: usize,
    pub window: AutoCompactWindowSnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutoCompactTokenStatus {
    pub active_context_tokens: usize,
    pub scope_tokens: usize,
    pub limit_tokens: usize,
    pub full_context_window_limit: Option<usize>,
    pub window_ordinal: Option<u64>,
    pub window_prefill_tokens: Option<usize>,
    pub full_context_window_limit_reached: bool,
    pub token_limit_reached: bool,
}

pub fn derived_auto_compact_limit(model_context_window: usize, trigger_ratio: f32) -> usize {
    ((model_context_window as f32) * trigger_ratio)
        .floor()
        .max(1.0) as usize
}

pub fn effective_auto_compact_limit(
    model_context_window: usize,
    trigger_ratio: f32,
    configured_limit: Option<usize>,
) -> usize {
    let derived = derived_auto_compact_limit(model_context_window, trigger_ratio);
    configured_limit
        .map(|limit| limit.min(derived))
        .unwrap_or(derived)
}

pub fn auto_compact_token_status(input: AutoCompactPolicyInput) -> AutoCompactTokenStatus {
    let limit_tokens = effective_auto_compact_limit(
        input.model_context_window,
        input.trigger_ratio,
        input.configured_limit,
    );

    match input.scope {
        AutoCompactTokenLimitScope::Total => {
            let scope_tokens = input.active_context_tokens;
            AutoCompactTokenStatus {
                active_context_tokens: input.active_context_tokens,
                scope_tokens,
                limit_tokens,
                full_context_window_limit: None,
                window_ordinal: None,
                window_prefill_tokens: None,
                full_context_window_limit_reached: false,
                token_limit_reached: scope_tokens >= limit_tokens,
            }
        }
        AutoCompactTokenLimitScope::BodyAfterPrefix => {
            let baseline = input
                .window
                .prefill_input_tokens
                .unwrap_or(input.active_context_tokens);
            let scope_tokens = input.active_context_tokens.saturating_sub(baseline);
            let full_context_window_limit_reached =
                input.active_context_tokens >= input.model_context_window;
            AutoCompactTokenStatus {
                active_context_tokens: input.active_context_tokens,
                scope_tokens,
                limit_tokens,
                full_context_window_limit: Some(input.model_context_window),
                window_ordinal: Some(input.window.ordinal),
                window_prefill_tokens: input.window.prefill_input_tokens,
                full_context_window_limit_reached,
                token_limit_reached: scope_tokens >= limit_tokens
                    || full_context_window_limit_reached,
            }
        }
    }
}
