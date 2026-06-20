use super::{AutoCompactTokenLimitScope, AutoCompactWindow, RequestTokenBaseline, TurnHost};
use crate::{
    ContextManager, EventMsg, ModelResponse, RolloutItem, TokenUsageState,
    emit_assistant_message_item, emit_event,
};
use anyhow::Result;

pub(super) async fn record_model_response<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    context_manager: &mut ContextManager,
    response: &ModelResponse,
) -> Result<()> {
    let assistant_response_item = context_manager.record_assistant_message(
        response.content.clone(),
        response.reasoning.clone(),
        response.tool_calls.clone(),
    );
    host.persist_rollout_items(
        conversation_id,
        &[RolloutItem::from(assistant_response_item)],
    )
    .await?;
    host.save_history(context_manager.history().clone()).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_model_response_event<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    response: &ModelResponse,
    had_streaming_assistant_item: bool,
    assistant_item_seq: &mut usize,
    events: &mut Vec<EventMsg>,
    on_event: &mut (impl FnMut(&EventMsg) + Send + ?Sized),
) {
    if !had_streaming_assistant_item
        && let Some(content) = response.content.clone()
        && !content.trim().is_empty()
    {
        emit_assistant_message_item(events, on_event, turn_id, &content, assistant_item_seq);
    }
    emit_event(
        events,
        on_event,
        EventMsg::ModelResponseReceived {
            turn_id: turn_id.to_string(),
            model_name: response.model_name.clone(),
            has_content: response.content.is_some(),
            tool_call_count: response.tool_calls.len(),
        },
    );
    host.audit_model_response_received(
        conversation_id,
        turn_id,
        response.model_name.as_deref(),
        response.content.is_some(),
        response.tool_calls.len(),
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn update_token_usage_from_response(
    turn_id: &str,
    response: &ModelResponse,
    model_context_window: u64,
    model_auto_compact_token_limit_scope: AutoCompactTokenLimitScope,
    final_budget_estimated_tokens: usize,
    token_usage_state: &mut TokenUsageState,
    request_baseline: &mut RequestTokenBaseline,
    auto_compact_window: &mut AutoCompactWindow,
    events: &mut Vec<EventMsg>,
    on_event: &mut (impl FnMut(&EventMsg) + Send + ?Sized),
) {
    if let Some(usage) = response.usage.clone() {
        token_usage_state.append_server_usage(usage.clone(), Some(model_context_window));
        *request_baseline = RequestTokenBaseline {
            server_context_tokens: Some(usage.total_tokens as usize),
            request_estimated_tokens: Some(final_budget_estimated_tokens),
        };
        if matches!(
            model_auto_compact_token_limit_scope,
            AutoCompactTokenLimitScope::BodyAfterPrefix
        ) {
            auto_compact_window.ensure_server_observed_prefill_from_usage(&usage);
        }
        emit_event(
            events,
            on_event,
            EventMsg::TokenUsageUpdated {
                turn_id: turn_id.to_string(),
                last_usage: usage,
                total_usage: token_usage_state.total_usage.clone(),
                model_context_window: token_usage_state.model_context_window,
                request_estimated_tokens: final_budget_estimated_tokens as u64,
            },
        );
    }
}
