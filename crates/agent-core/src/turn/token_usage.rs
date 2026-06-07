use crate::model::ModelUsage;
use crate::rollout::RolloutItem;
use crate::turn::{AutoCompactWindow, AutoCompactWindowSnapshot, EventMsg};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenUsageState {
    pub total_usage: ModelUsage,
    pub last_usage: Option<ModelUsage>,
    pub model_context_window: Option<u64>,
}

impl TokenUsageState {
    pub fn restore(
        total_usage: ModelUsage,
        last_usage: Option<ModelUsage>,
        model_context_window: Option<u64>,
    ) -> Self {
        Self {
            total_usage,
            last_usage,
            model_context_window,
        }
    }

    pub fn append_server_usage(&mut self, usage: ModelUsage, model_context_window: Option<u64>) {
        self.total_usage.add_assign(&usage);
        self.last_usage = Some(usage);
        if model_context_window.is_some() {
            self.model_context_window = model_context_window;
        }
    }

    pub fn total_tokens(&self) -> usize {
        self.total_usage.total_tokens as usize
    }

    pub fn active_context_tokens_from_last_usage(&self) -> Option<usize> {
        self.last_usage
            .as_ref()
            .map(|usage| usage.total_tokens as usize)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RequestTokenBaseline {
    pub server_context_tokens: Option<usize>,
    pub request_estimated_tokens: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoredTurnTokenState {
    pub usage: TokenUsageState,
    pub request_baseline: RequestTokenBaseline,
    pub auto_compact_window: AutoCompactWindowSnapshot,
}

impl Default for RestoredTurnTokenState {
    fn default() -> Self {
        Self {
            usage: TokenUsageState::default(),
            request_baseline: RequestTokenBaseline::default(),
            auto_compact_window: AutoCompactWindowSnapshot::default(),
        }
    }
}

pub fn latest_turn_token_state_from_rollout_items(
    rollout_items: &[RolloutItem],
) -> Option<RestoredTurnTokenState> {
    let mut restored = RestoredTurnTokenState::default();
    let mut saw_usage_or_compaction = false;
    let mut window = AutoCompactWindow::new();

    for item in rollout_items {
        match item {
            RolloutItem::EventMsg {
                event:
                    EventMsg::TokenUsageUpdated {
                        last_usage,
                        total_usage,
                        request_estimated_tokens,
                        model_context_window,
                        ..
                    },
            } => {
                saw_usage_or_compaction = true;
                restored.usage = TokenUsageState::restore(
                    total_usage.clone(),
                    Some(last_usage.clone()),
                    *model_context_window,
                );
                restored.request_baseline = RequestTokenBaseline {
                    server_context_tokens: Some(last_usage.total_tokens as usize),
                    request_estimated_tokens: Some(*request_estimated_tokens as usize),
                };
            }
            RolloutItem::EventMsg {
                event:
                    EventMsg::ContextCompacted {
                        post_context_tokens_estimate,
                        ..
                    },
            } => {
                saw_usage_or_compaction = true;
                let tokens = *post_context_tokens_estimate as usize;
                restored.request_baseline = RequestTokenBaseline {
                    server_context_tokens: Some(tokens),
                    request_estimated_tokens: Some(tokens),
                };
                window.start_next();
                window.set_estimated_prefill(tokens);
                restored.auto_compact_window = window.snapshot();
            }
            _ => {}
        }
    }

    saw_usage_or_compaction.then_some(restored)
}

pub fn apply_signed_token_delta(base: usize, current: usize, previous: usize) -> usize {
    if current >= previous {
        base.saturating_add(current - previous)
    } else {
        base.saturating_sub(previous - current)
    }
}

#[cfg(test)]
#[path = "token_usage_tests.rs"]
mod token_usage_tests;
