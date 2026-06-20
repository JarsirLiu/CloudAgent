use crate::conversation::ResponseItem;

use super::support::{estimate_message_tokens, render_input_items_for_compaction};

#[derive(Clone, Copy, Debug)]
pub struct ContextCompactionConfig {
    pub model_context_window: u64,
    pub trigger_ratio: f32,
    pub compacted_target_tokens: usize,
    pub preserved_user_turns: usize,
    pub preserved_tail_tokens: usize,
    pub summary_source_max_tokens: usize,
}

#[derive(Clone, Debug)]
pub struct ContextCompactionPlan {
    pub(crate) prefix: Vec<ResponseItem>,
    pub(crate) preserved_tail: Vec<ResponseItem>,
}

pub fn plan_history_compaction(
    messages: &[ResponseItem],
    config: ContextCompactionConfig,
) -> Option<ContextCompactionPlan> {
    let estimated = estimate_message_tokens(messages);
    if messages.len() <= 6 {
        return None;
    }
    let trigger_tokens = ((config.model_context_window as f32) * config.trigger_ratio) as usize;
    let available_history_tokens = trigger_tokens.max(1);
    if estimated <= available_history_tokens {
        return None;
    }

    build_compaction_plan(messages, config)
}

pub fn plan_manual_history_compaction(
    messages: &[ResponseItem],
    config: ContextCompactionConfig,
    minimum_history_tokens: usize,
) -> Option<ContextCompactionPlan> {
    if messages.len() <= 6 {
        return None;
    }

    let estimated = estimate_message_tokens(messages);
    if estimated < minimum_history_tokens.max(1) {
        return None;
    }

    build_compaction_plan(messages, config)
}

fn build_compaction_plan(
    messages: &[ResponseItem],
    config: ContextCompactionConfig,
) -> Option<ContextCompactionPlan> {
    let mut keep_start = choose_tail_start(
        messages,
        config.preserved_user_turns.max(1),
        config.preserved_tail_tokens.max(1),
    );
    keep_start = adjust_tail_start_for_tool_invariants(messages, keep_start);

    if keep_start <= 1 || keep_start >= messages.len() {
        keep_start =
            choose_tail_start_from_token_budget(messages, config.compacted_target_tokens.max(1));
        keep_start = adjust_tail_start_for_tool_invariants(messages, keep_start);
    }

    if keep_start <= 1 || keep_start >= messages.len() {
        return None;
    }

    let prefix = messages[1..keep_start].to_vec();
    if prefix.is_empty() {
        return None;
    }

    Some(ContextCompactionPlan {
        prefix,
        preserved_tail: messages[keep_start..].to_vec(),
    })
}

pub(crate) fn choose_tail_start(
    messages: &[ResponseItem],
    preserved_user_turns: usize,
    tail_budget: usize,
) -> usize {
    let candidate = find_recent_user_boundary(messages, preserved_user_turns).unwrap_or(1);
    if estimate_message_tokens(&messages[candidate..]) <= tail_budget {
        return candidate;
    }

    let mut remaining_turns = preserved_user_turns.saturating_sub(1);
    while remaining_turns > 0 {
        let fallback = find_recent_user_boundary(messages, remaining_turns).unwrap_or(candidate);
        if estimate_message_tokens(&messages[fallback..]) <= tail_budget {
            return fallback;
        }
        remaining_turns -= 1;
    }

    choose_tail_start_from_token_budget(messages, tail_budget)
}

fn find_recent_user_boundary(
    messages: &[ResponseItem],
    preserved_user_turns: usize,
) -> Option<usize> {
    let mut seen_users = 0usize;
    for index in (1..messages.len()).rev() {
        if response_item_is_real_user_message(&messages[index]) {
            seen_users += 1;
            if seen_users == preserved_user_turns {
                return Some(index);
            }
        }
    }
    None
}

fn choose_tail_start_from_token_budget(messages: &[ResponseItem], target_limit: usize) -> usize {
    let mut keep_start = messages.len();
    let mut kept_tokens = 0usize;

    for index in (1..messages.len()).rev() {
        let item_tokens = estimate_message_tokens(std::slice::from_ref(&messages[index]));
        if kept_tokens.saturating_add(item_tokens) > target_limit && keep_start < messages.len() {
            break;
        }

        keep_start = index;
        kept_tokens = kept_tokens.saturating_add(item_tokens);
    }

    keep_start.max(1)
}

pub(crate) fn adjust_tail_start_for_tool_invariants(
    messages: &[ResponseItem],
    mut keep_start: usize,
) -> usize {
    loop {
        let mut changed = false;
        let mut missing_call_index = None;
        for item in &messages[keep_start..] {
            let ResponseItem::Tool { tool_call_id, .. } = item else {
                continue;
            };
            let call_index = find_matching_tool_call(messages, tool_call_id);
            if let Some(index) = call_index
                && index < keep_start
            {
                missing_call_index = Some(index);
                break;
            }
        }

        if let Some(index) = missing_call_index {
            keep_start = index;
            changed = true;
        }

        if !changed {
            break;
        }
    }

    keep_start
}

fn find_matching_tool_call(messages: &[ResponseItem], tool_call_id: &str) -> Option<usize> {
    for index in (1..messages.len()).rev() {
        let ResponseItem::Assistant { tool_calls, .. } = &messages[index] else {
            continue;
        };
        if tool_calls.iter().any(|call| call.id == tool_call_id) {
            return Some(index);
        }
    }
    None
}

fn response_item_is_real_user_message(item: &ResponseItem) -> bool {
    let ResponseItem::User { content } = item else {
        return false;
    };
    let text = render_input_items_for_compaction(content);
    let trimmed = text.trim_start();
    !trimmed.starts_with("[Context Summary]") && !trimmed.starts_with("<turn_aborted>")
}
