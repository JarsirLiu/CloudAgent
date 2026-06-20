use super::context::{
    BudgetedFragmentInputs, append_rendered_fragments, build_budgeted_fragments_for_current_history,
};
use super::flow::{AppliedCompaction, CompactionMode, maybe_compact_history_with_start_callback};
use super::prepare::PreparedTurnContext;
use super::window::AutoCompactWindow;
use crate::context::{ContextFacade, ContextManager, FilterPolicy};
use crate::rollout::RolloutItem;
use crate::turn::{AutoCompactTokenStatus, RequestTokenBaseline, TurnHost};
use crate::{EventMsg, emit_event};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(crate) struct AutoCompactionLifecycleInput<'a, H: TurnHost> {
    pub host: &'a H,
    pub conversation_id: &'a str,
    pub turn_id: &'a str,
    pub cancellation_token: &'a CancellationToken,
    pub context_facade: &'a ContextFacade,
    pub context_manager: &'a mut ContextManager,
    pub filter_policy: FilterPolicy,
    pub environment_context: &'a crate::context::EnvironmentContext,
    pub settings: &'a crate::turn::ChatTurnSettings,
    pub turn_explicit_skill_fragments: &'a [crate::ResponseItem],
    pub raw_memory_fragment: &'a Option<String>,
    pub skill_summary: &'a Option<String>,
    pub request_baseline: &'a mut RequestTokenBaseline,
    pub auto_compact_window: &'a mut AutoCompactWindow,
    pub token_status: AutoCompactTokenStatus,
    pub active_context_tokens_estimate: usize,
    pub roundtrip_count: usize,
    pub events: &'a mut Vec<EventMsg>,
    pub on_event: &'a mut (dyn for<'b> FnMut(&'b EventMsg) + Send + 'a),
}

pub(crate) async fn prepare_context_with_auto_compaction<H>(
    input: AutoCompactionLifecycleInput<'_, H>,
) -> Result<PreparedTurnContext>
where
    H: TurnHost,
{
    let AutoCompactionLifecycleInput {
        host,
        conversation_id,
        turn_id,
        cancellation_token,
        context_facade,
        context_manager,
        filter_policy,
        environment_context,
        settings,
        turn_explicit_skill_fragments,
        raw_memory_fragment,
        skill_summary,
        request_baseline,
        auto_compact_window,
        token_status,
        active_context_tokens_estimate,
        roundtrip_count,
        events,
        on_event,
    } = input;

    let budgeted_before_compaction = build_budgeted_fragments_for_current_history(
        context_facade,
        context_manager,
        filter_policy,
        environment_context,
        settings,
        BudgetedFragmentInputs {
            raw_memory_fragment: raw_memory_fragment.clone(),
            skill_summary: skill_summary.clone(),
        },
    );

    let compaction = maybe_compact_history_with_start_callback(
        host,
        context_manager.history_mut(),
        cancellation_token,
        CompactionMode::Automatic {
            estimated_total_tokens: token_status.active_context_tokens,
            token_limit_reached: token_status.token_limit_reached,
            phase: super::prepare::compaction_phase(roundtrip_count),
        },
        |start| {
            emit_event(
                events,
                on_event,
                EventMsg::ContextCompactionStarted {
                    turn_id: turn_id.to_string(),
                    trigger: start.trigger,
                    reason: start.reason,
                    phase: start.phase,
                    estimated_tokens: token_status
                        .active_context_tokens
                        .max(start.estimated_history_tokens)
                        as u64,
                },
            );
        },
    )
    .await?;

    if let Some(compacted) = compaction.as_ref() {
        persist_compaction_checkpoint(
            host,
            conversation_id,
            turn_id,
            context_manager,
            compacted,
            request_baseline,
            auto_compact_window,
            events,
            on_event,
        )
        .await?;
    }

    let budgeted = if compaction.is_some() {
        build_budgeted_fragments_for_current_history(
            context_facade,
            context_manager,
            filter_policy,
            environment_context,
            settings,
            BudgetedFragmentInputs {
                raw_memory_fragment: raw_memory_fragment.clone(),
                skill_summary: skill_summary.clone(),
            },
        )
    } else {
        budgeted_before_compaction
    };
    let prepared_fragments =
        append_rendered_fragments(budgeted.fragments.clone(), turn_explicit_skill_fragments);

    Ok(PreparedTurnContext {
        budgeted,
        prepared_fragments,
        token_status,
        active_context_tokens_estimate,
        compaction,
    })
}

#[allow(clippy::too_many_arguments)]
async fn persist_compaction_checkpoint<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    context_manager: &ContextManager,
    compacted: &AppliedCompaction,
    request_baseline: &mut RequestTokenBaseline,
    auto_compact_window: &mut AutoCompactWindow,
    events: &mut Vec<EventMsg>,
    on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
) -> Result<()> {
    let post_context_tokens = compacted.post_context_tokens_estimate as usize;
    *request_baseline = RequestTokenBaseline {
        server_context_tokens: Some(post_context_tokens),
        request_estimated_tokens: Some(post_context_tokens),
    };
    auto_compact_window.start_next();
    auto_compact_window.set_estimated_prefill(post_context_tokens);
    host.persist_rollout_items(
        conversation_id,
        &[RolloutItem::Compacted {
            summary: compacted.summary.clone(),
            rendered_summary: compacted.rendered_summary.clone(),
            trigger: compacted.trigger,
            reason: compacted.reason,
            phase: compacted.phase,
            replacement_history: compacted.replacement_history.clone(),
        }],
    )
    .await?;
    host.save_history(context_manager.history().clone()).await?;
    emit_event(
        events,
        on_event,
        EventMsg::ContextCompacted {
            turn_id: turn_id.to_string(),
            trigger: compacted.trigger,
            reason: compacted.reason,
            phase: compacted.phase,
            pre_context_tokens_estimate: compacted.pre_context_tokens_estimate,
            post_context_tokens_estimate: compacted.post_context_tokens_estimate,
            pre_message_count: compacted.pre_message_count,
            post_message_count: compacted.post_message_count,
            preserved_user_count: compacted.preserved_user_count,
        },
    );
    Ok(())
}
