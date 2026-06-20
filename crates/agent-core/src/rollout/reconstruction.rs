use crate::context::{
    append_turn_aborted_marker_if_needed, counts_as_real_user_turn, is_context_summary_item,
};
use crate::conversation::{ConversationHistory, ResponseItem};
use crate::rollout::RolloutItem;
use crate::turn::{EventMsg, TurnState};

pub fn conversation_history_from_rollout_items(
    conversation_id: impl Into<String>,
    system_prompt: impl Into<String>,
    items: &[RolloutItem],
) -> ConversationHistory {
    let conversation_id = conversation_id.into();
    let system_prompt = system_prompt.into();
    let (mut history, suffix) =
        latest_compaction_checkpoint(&conversation_id, &system_prompt, items).unwrap_or_else(
            || {
                (
                    ConversationHistory::new(conversation_id.clone(), system_prompt.clone()),
                    items,
                )
            },
        );

    replay_model_history_suffix(&mut history, suffix);
    history.ensure_tool_outputs_present();
    history
}

fn latest_compaction_checkpoint<'a>(
    conversation_id: &str,
    system_prompt: &str,
    items: &'a [RolloutItem],
) -> Option<(ConversationHistory, &'a [RolloutItem])> {
    let (index, replacement_history) =
        items
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, item)| match item {
                RolloutItem::Compacted {
                    replacement_history,
                    ..
                } if !replacement_history.is_empty() => Some((index, replacement_history)),
                _ => None,
            })?;

    let replacement_history = normalize_compacted_replacement_history(replacement_history);
    let mut history = ConversationHistory::new(conversation_id.to_string(), system_prompt);
    history.messages = replacement_history.clone();
    history.turn_count = replacement_history
        .iter()
        .filter(|item| counts_as_real_user_turn(item))
        .count() as u64;

    Some((history, &items[index + 1..]))
}

fn replay_model_history_suffix(history: &mut ConversationHistory, items: &[RolloutItem]) {
    let mut pending_turn: Option<PendingModelHistoryTurn> = None;
    for item in items {
        match item {
            RolloutItem::ResponseItem { item } => match item {
                ResponseItem::System { content } => {
                    flush_pending_model_history_turn(history, &mut pending_turn);
                    history.messages[0] = ResponseItem::System {
                        content: content.clone(),
                    };
                }
                ResponseItem::User { content } => {
                    flush_pending_model_history_turn(history, &mut pending_turn);
                    pending_turn = Some(PendingModelHistoryTurn {
                        items: vec![ResponseItem::User {
                            content: content.clone(),
                        }],
                        has_model_output: false,
                        terminal_state: None,
                    });
                }
                ResponseItem::Assistant {
                    content,
                    reasoning,
                    tool_calls,
                } => {
                    let item = ResponseItem::Assistant {
                        content: content.clone(),
                        reasoning: reasoning.clone(),
                        tool_calls: tool_calls.clone(),
                    };
                    if let Some(turn) = pending_turn.as_mut() {
                        turn.has_model_output = true;
                        turn.items.push(item);
                    } else {
                        history.messages.push(item);
                    }
                }
                ResponseItem::Tool {
                    tool_call_id,
                    name,
                    content,
                    structured,
                } => {
                    let item = ResponseItem::Tool {
                        tool_call_id: tool_call_id.clone(),
                        name: name.clone(),
                        content: content.clone(),
                        structured: structured.clone(),
                    };
                    if let Some(turn) = pending_turn.as_mut() {
                        turn.has_model_output = true;
                        turn.items.push(item);
                    } else {
                        history.messages.push(item);
                    }
                }
            },
            RolloutItem::EventMsg { event } => match event {
                EventMsg::TurnCompleted { .. } => {
                    if let Some(turn) = pending_turn.as_mut() {
                        turn.terminal_state = Some(TurnState::Completed);
                    }
                    flush_pending_model_history_turn(history, &mut pending_turn);
                }
                EventMsg::TurnFailed { .. } => {
                    if let Some(turn) = pending_turn.as_mut() {
                        turn.terminal_state = Some(TurnState::Failed);
                    }
                    flush_pending_model_history_turn(history, &mut pending_turn);
                }
                EventMsg::TurnCancelled { .. } => {
                    if let Some(turn) = pending_turn.as_mut() {
                        turn.terminal_state = Some(TurnState::Cancelled);
                    } else {
                        append_turn_aborted_marker_if_needed(history);
                    }
                    flush_pending_model_history_turn(history, &mut pending_turn);
                }
                EventMsg::TurnStarted { .. }
                | EventMsg::ModelRequestStarted { .. }
                | EventMsg::ModelResponseReceived { .. }
                | EventMsg::ModelRetrying { .. }
                | EventMsg::TokenUsageUpdated { .. }
                | EventMsg::ContextCompactionStarted { .. }
                | EventMsg::ContextCompacted { .. }
                | EventMsg::ItemStarted { .. }
                | EventMsg::ItemDelta { .. }
                | EventMsg::ItemCompleted { .. }
                | EventMsg::ServerRequestRequested { .. }
                | EventMsg::ServerRequestResolved { .. } => {}
            },
            RolloutItem::Compacted { .. } => {}
        }
    }
    flush_pending_model_history_turn(history, &mut pending_turn);
}

struct PendingModelHistoryTurn {
    items: Vec<ResponseItem>,
    has_model_output: bool,
    terminal_state: Option<TurnState>,
}

fn flush_pending_model_history_turn(
    history: &mut ConversationHistory,
    pending_turn: &mut Option<PendingModelHistoryTurn>,
) {
    let Some(turn) = pending_turn.take() else {
        return;
    };
    if matches!(
        turn.terminal_state,
        Some(TurnState::Cancelled | TurnState::Failed)
    ) && !turn.has_model_output
    {
        return;
    }
    let append_turn_aborted_marker = matches!(turn.terminal_state, Some(TurnState::Cancelled));
    history.turn_count += turn
        .items
        .iter()
        .filter(|item| counts_as_real_user_turn(item))
        .count() as u64;
    history.messages.extend(turn.items);
    if append_turn_aborted_marker {
        append_turn_aborted_marker_if_needed(history);
    }
}

fn normalize_compacted_replacement_history(
    replacement_history: &[ResponseItem],
) -> Vec<ResponseItem> {
    replacement_history
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            ResponseItem::System { content } if index > 0 && is_context_summary_item(item) => {
                ResponseItem::User {
                    content: crate::text_input_items(content.clone()),
                }
            }
            _ => item.clone(),
        })
        .collect()
}
