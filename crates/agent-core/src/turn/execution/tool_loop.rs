use super::completion::{
    TurnCompletionContext, cancelled_outcome, completed_outcome, emit_turn_completed,
    fail_turn_with_message,
};
use super::state::ChatTurnState;
use super::tooling::finish_reason_implies_tool_use;
use super::{ServerRequestHandler, ToolBatchOutcome, TurnHost, TurnOutcome};
use crate::{EventMsg, ToolSpec, emit_assistant_message_item};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(super) enum ToolLoopControl {
    Continue,
    Return(TurnOutcome),
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn advance_after_model_response<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    permission_profile: &H::PermissionProfile,
    approval_policy: &H::ApprovalPolicy,
    cancellation_token: CancellationToken,
    response: &crate::ModelResponse,
    had_streaming_assistant_item: bool,
    tool_specs: &[ToolSpec],
    discoverable_tools: &[ToolSpec],
    state: &mut ChatTurnState,
    on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
    approval: &dyn ServerRequestHandler,
) -> Result<ToolLoopControl> {
    let tool_calls = response.tool_calls.clone();
    let finish_reason = response.finish_reason.as_deref();

    if tool_calls.is_empty() && !finish_reason_implies_tool_use(finish_reason) {
        state.loop_guard.reset();
        state.tool_only_roundtrips_after_compaction = 0;
        if !had_streaming_assistant_item && response.content.is_none() {
            emit_assistant_message_item(
                &mut state.events,
                on_event,
                turn_id,
                "The model returned an empty response.",
                &mut state.assistant_item_seq,
            );
        }
        emit_turn_completed(&mut state.events, on_event, turn_id);
        return Ok(ToolLoopControl::Return(completed_outcome(
            turn_id,
            std::mem::take(&mut state.events),
            state.context_manager.history().clone(),
            state.last_model_name.clone(),
        )));
    }

    if tool_calls.is_empty() && finish_reason_implies_tool_use(finish_reason) {
        state.loop_guard.reset();
        return Ok(ToolLoopControl::Continue);
    }

    if state.saw_compaction_this_turn
        && response
            .content
            .as_ref()
            .is_none_or(|content| content.trim().is_empty())
    {
        state.tool_only_roundtrips_after_compaction += 1;
        if state.tool_only_roundtrips_after_compaction
            > host
                .chat_turn_settings()
                .max_tool_only_roundtrips_after_compaction
        {
            let outcome = fail_turn_with_message(
                TurnCompletionContext {
                    host,
                    conversation_id,
                    turn_id,
                    context_manager: &mut state.context_manager,
                    events: &mut state.events,
                    on_event,
                    assistant_item_seq: &mut state.assistant_item_seq,
                    model_name: state.last_model_name.clone(),
                },
                "Stopped after automatic compaction because the model continued requesting tools without producing an answer. Please retry or narrow the request.".to_string(),
            )
            .await?;
            return Ok(ToolLoopControl::Return(outcome));
        }
    } else {
        state.tool_only_roundtrips_after_compaction = 0;
    }

    let tool_batch: ToolBatchOutcome = host
        .run_tool_batch(
            conversation_id,
            turn_id,
            permission_profile,
            approval_policy,
            cancellation_token,
            tool_calls.clone(),
            tool_specs,
            discoverable_tools,
            &mut state.context_manager,
            &mut state.events,
            on_event,
            approval,
            &mut state.denied_requests,
        )
        .await?;
    if tool_batch.cancelled {
        return Ok(ToolLoopControl::Return(cancelled_outcome(
            turn_id,
            std::mem::take(&mut state.events),
            state.context_manager.history().clone(),
            state.last_model_name.clone(),
        )));
    }

    state.exposed_tool_names = tool_batch.exposed_tools;
    if let Some(loop_abort) = state.loop_guard.record_roundtrip(
        &tool_calls,
        tool_specs,
        &state.context_manager.history().messages,
    ) {
        let message = format!(
            "Stopped this turn because the model entered a repetitive loop: the same read-only tool calls and results repeated {} times without progress.",
            loop_abort.repeated_count
        );
        let outcome = fail_turn_with_message(
            TurnCompletionContext {
                host,
                conversation_id,
                turn_id,
                context_manager: &mut state.context_manager,
                events: &mut state.events,
                on_event,
                assistant_item_seq: &mut state.assistant_item_seq,
                model_name: state.last_model_name.clone(),
            },
            message,
        )
        .await?;
        return Ok(ToolLoopControl::Return(outcome));
    }

    Ok(ToolLoopControl::Continue)
}
