use super::compaction::prepare_turn_context_with_auto_compaction;
use super::completion::{
    TurnCompletionContext, cancelled_outcome, complete_turn_with_message, emit_turn_cancelled,
    fail_turn_with_message,
};
use super::response::{
    handle_model_response_event, record_model_response, update_token_usage_from_response,
};
use super::state::ChatTurnState;
use super::streaming::observe_model_response_stream;
use super::tool_loop::{ToolLoopControl, advance_after_model_response};
use super::tooling::{collect_discoverable_tools, compose_visible_tool_specs};
use super::{ServerRequestHandler, TurnHost, TurnOutcome, build_model_request_shape_audit};
use crate::EventMsg;
use crate::context::ContextFragment;
use crate::context::{append_turn_aborted_marker_if_needed, turn_aborted_marker_item};
use crate::{ContextBudgetLogEntry, append_context_budget_log, emit_event};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

#[allow(clippy::too_many_arguments)]
pub async fn execute_chat_turn<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    permission_profile: &H::PermissionProfile,
    approval_policy: &H::ApprovalPolicy,
    cancellation_token: CancellationToken,
    history: crate::ConversationHistory,
    on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
    approval: &dyn ServerRequestHandler,
) -> Result<TurnOutcome> {
    let settings = host.chat_turn_settings();
    let mut state = ChatTurnState::new(host, conversation_id, permission_profile, history).await?;
    let environment_context = host.environment_context();
    let raw_memory_fragment = host.raw_memory_fragment();

    let mut roundtrip_count = 0usize;
    loop {
        if let Some(limit) = settings.max_tool_roundtrips
            && roundtrip_count >= limit
        {
            break;
        }
        roundtrip_count += 1;
        let tool_specs = compose_visible_tool_specs(
            &host
                .resolve_chat_turn_tool_exposure(permission_profile)
                .default_tools,
            &state.deferred_tool_map,
            &state.exposed_tool_names,
        );
        let discoverable_tools =
            collect_discoverable_tools(&state.deferred_tool_map, &state.exposed_tool_names);
        if cancellation_token.is_cancelled() || host.is_turn_cancelled(conversation_id).await {
            persist_turn_aborted_marker(host, conversation_id, &mut state.context_manager).await?;
            emit_turn_cancelled(
                &mut state.events,
                on_event,
                turn_id,
                "interrupted by client",
            );
            return Ok(cancelled_outcome(
                turn_id,
                state.events,
                state.context_manager.history().clone(),
                state.last_model_name,
            ));
        }

        let prepared_context = prepare_turn_context_with_auto_compaction(
            host,
            conversation_id,
            turn_id,
            &cancellation_token,
            &state.context_facade,
            &mut state.context_manager,
            state.filter_policy,
            &environment_context,
            &settings,
            &tool_specs,
            &state.turn_explicit_skill_fragments,
            &raw_memory_fragment,
            &state.skill_summary,
            &mut state.request_baseline,
            &mut state.auto_compact_window,
            roundtrip_count,
            &mut state.events,
            on_event,
        )
        .await?;
        let compaction = prepared_context.compaction.clone();
        if compaction.is_some() {
            state.saw_compaction_this_turn = true;
            state.tool_only_roundtrips_after_compaction = 0;
        }
        let budgeted = prepared_context.budgeted;
        let prepared_fragments = prepared_context.prepared_fragments;
        let token_status = prepared_context.token_status;
        let active_context_tokens_estimate = prepared_context.active_context_tokens_estimate;
        let model_request = state
            .context_facade
            .prepare_model_request(
                &state.context_manager,
                &settings.workspace_root,
                state.filter_policy,
                prepared_fragments,
                tool_specs.clone(),
                settings.llm_temperature,
            )
            .model_request;
        let mut model_request = model_request;
        model_request.tool_output_token_limit = settings.tool_output_token_limit;
        let final_budget = state.context_facade.check_final_model_request_budget(
            &model_request,
            settings.model_context_window as usize,
            settings.context_budget_safety_buffer_tokens,
        );
        let history_tokens_now = state.context_facade.estimate_history_tokens_for_compaction(
            &state.context_manager.history().messages,
            state.filter_policy,
            &settings.workspace_root,
        );
        let trigger_tokens = token_status.limit_tokens;
        let overhead_now = state.context_facade.estimate_request_overhead_tokens(
            &state.context_manager.history().messages,
            &environment_context.render(),
            &tool_specs,
            settings.context_compaction_request_overhead_tokens,
        );
        let compaction_triggered_now = token_status.token_limit_reached;
        let _ = append_context_budget_log(
            &settings.data_root_dir,
            &ContextBudgetLogEntry {
                conversation_id: conversation_id.to_string(),
                turn_id: turn_id.to_string(),
                model_context_window: settings.model_context_window,
                trigger_ratio: settings.context_compaction_trigger_ratio,
                trigger_tokens,
                estimated_total_tokens: active_context_tokens_estimate,
                filter_enabled: settings.pre_llm_filter_enabled,
                sdk_total_tokens: state.request_baseline.server_context_tokens,
                history_tokens: history_tokens_now,
                overhead_tokens: overhead_now,
                memory_floor_tokens: settings.post_compact_memory_floor_tokens,
                safety_buffer_tokens: settings.context_budget_safety_buffer_tokens,
                compaction_triggered: compaction_triggered_now,
                hard_cap_triggered: budgeted.audit.hard_cap_triggered || final_budget.exceeded,
                memory_before: budgeted.audit.memory_before,
                memory_after: budgeted.audit.memory_after,
                skills_before: budgeted.audit.skills_before,
                skills_after: budgeted.audit.skills_after,
                mcp_before: budgeted.audit.mcp_before,
                mcp_after: budgeted.audit.mcp_after,
            },
        );
        if final_budget.exceeded {
            let message = format!(
                "Stopped before sending the model request because the final input context exceeded the budget (estimated {} tokens > limit {}). Narrow the request context or strengthen input filtering before retrying.",
                final_budget.estimated_tokens, final_budget.limit_tokens
            );
            return fail_turn_with_message(
                TurnCompletionContext {
                    host,
                    conversation_id,
                    turn_id,
                    context_manager: &mut state.context_manager,
                    events: &mut state.events,
                    on_event,
                    assistant_item_seq: &mut state.assistant_item_seq,
                    model_name: state.last_model_name,
                },
                message,
            )
            .await;
        }

        emit_event(
            &mut state.events,
            on_event,
            EventMsg::ModelRequestStarted {
                turn_id: turn_id.to_string(),
                message_count: model_request.messages.len(),
                tool_count: tool_specs.len(),
            },
        );
        host.audit_model_request_started(
            conversation_id,
            turn_id,
            model_request.messages.len(),
            tool_specs.len(),
        );
        let request_shape = build_model_request_shape_audit(
            &model_request.messages,
            tool_specs.len(),
            compaction.as_ref().map(|compacted| compacted.phase),
        );
        host.audit_model_request_shape(conversation_id, turn_id, &request_shape);

        let streamed_response = observe_model_response_stream(
            host,
            &cancellation_token,
            model_request,
            turn_id,
            &mut state.assistant_item_seq,
            &mut state.reasoning_item_seq,
            &mut state.events,
            on_event,
        )
        .await?;
        let had_streaming_assistant_item = streamed_response.had_streaming_assistant_item;
        let response = streamed_response.response;

        state.last_model_name = response.model_name.clone();
        handle_model_response_event(
            host,
            conversation_id,
            turn_id,
            &response,
            had_streaming_assistant_item,
            &mut state.assistant_item_seq,
            &mut state.events,
            on_event,
        );
        update_token_usage_from_response(
            turn_id,
            &response,
            settings.model_context_window,
            settings.model_auto_compact_token_limit_scope,
            final_budget.estimated_tokens,
            &mut state.token_usage_state,
            &mut state.request_baseline,
            &mut state.auto_compact_window,
            &mut state.events,
            on_event,
        );
        record_model_response(host, conversation_id, &mut state.context_manager, &response).await?;
        match advance_after_model_response(
            host,
            conversation_id,
            turn_id,
            permission_profile,
            approval_policy,
            cancellation_token.clone(),
            &response,
            had_streaming_assistant_item,
            &tool_specs,
            &discoverable_tools,
            &mut state,
            on_event,
            approval,
        )
        .await?
        {
            ToolLoopControl::Continue => {}
            ToolLoopControl::Return(outcome) => return Ok(outcome),
        }
    }

    let roundtrip_limit_message =
        "Reached the configured tool roundtrip limit before the model produced a final answer."
            .to_string();
    complete_turn_with_message(
        TurnCompletionContext {
            host,
            conversation_id,
            turn_id,
            context_manager: &mut state.context_manager,
            events: &mut state.events,
            on_event,
            assistant_item_seq: &mut state.assistant_item_seq,
            model_name: state.last_model_name,
        },
        roundtrip_limit_message,
    )
    .await
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;

async fn persist_turn_aborted_marker<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    context_manager: &mut crate::ContextManager,
) -> Result<()> {
    append_turn_aborted_marker_if_needed(context_manager.history_mut());
    host.persist_rollout_items(
        conversation_id,
        &[crate::RolloutItem::from(turn_aborted_marker_item())],
    )
    .await?;
    host.save_history(context_manager.history().clone()).await?;
    Ok(())
}
