use super::context::{
    BudgetedFragmentInputs, append_rendered_fragments, build_budgeted_fragments_for_current_history,
};
use super::flow::{
    AppliedCompaction, CompactionContinuation, CompactionMode,
    maybe_compact_history_with_start_callback,
};
use super::policy::{AutoCompactPolicyInput, AutoCompactTokenStatus, auto_compact_token_status};
use super::window::AutoCompactWindow;
use crate::context::{ContextFacade, ContextInjectionStrategy, ContextManager, FilterPolicy};
use crate::rollout::RolloutItem;
use crate::turn::{RequestTokenBaseline, TurnHost, apply_signed_token_delta};
use crate::{EventMsg, emit_event};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(crate) struct PreparedTurnContext {
    pub budgeted: crate::context::BudgetedFragments,
    pub prepared_fragments: Vec<crate::ResponseItem>,
    pub injection_strategy: ContextInjectionStrategy,
    pub token_status: AutoCompactTokenStatus,
    pub active_context_tokens_estimate: usize,
    pub compaction: Option<AppliedCompaction>,
}

pub(crate) fn compaction_continuation(roundtrip_count: usize) -> CompactionContinuation {
    if roundtrip_count <= 1 {
        CompactionContinuation::PreTurn
    } else {
        CompactionContinuation::MidTurn
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn prepare_turn_context_with_auto_compaction<H>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    cancellation_token: &CancellationToken,
    context_facade: &ContextFacade,
    context_manager: &mut ContextManager,
    filter_policy: FilterPolicy,
    environment_context: &crate::context::EnvironmentContext,
    settings: &crate::turn::ChatTurnSettings,
    tool_specs: &[crate::ToolSpec],
    turn_explicit_skill_fragments: &[crate::ResponseItem],
    raw_memory_fragment: &Option<String>,
    skill_summary: &Option<String>,
    request_baseline: &mut RequestTokenBaseline,
    auto_compact_window: &mut AutoCompactWindow,
    roundtrip_count: usize,
    events: &mut Vec<EventMsg>,
    on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
) -> Result<PreparedTurnContext>
where
    H: TurnHost,
{
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
    let candidate_fragments = append_rendered_fragments(
        budgeted_before_compaction.fragments.clone(),
        turn_explicit_skill_fragments,
    );
    let mut candidate_request = context_manager
        .build_current_model_request_with_rendered_fragments(
            &candidate_fragments,
            ContextInjectionStrategy::Standard,
            tool_specs.to_vec(),
            settings.llm_temperature,
        );
    candidate_request.tool_output_token_limit = settings.tool_output_token_limit;
    candidate_request.messages = context_facade.apply_pre_llm_filter(
        candidate_request.messages,
        filter_policy,
        &settings.workspace_root,
    );
    let candidate_request_tokens = context_facade.estimate_model_request_tokens(&candidate_request);
    let active_context_tokens_estimate = match (
        request_baseline.server_context_tokens,
        request_baseline.request_estimated_tokens,
    ) {
        (Some(server_tokens), Some(previous_request_tokens)) => apply_signed_token_delta(
            server_tokens,
            candidate_request_tokens,
            previous_request_tokens,
        ),
        _ => candidate_request_tokens,
    };
    let compaction_estimated_total_tokens = context_facade
        .estimate_history_tokens_for_canonical_compaction(
            &context_manager.history().messages,
            &settings.workspace_root,
        );
    let active_context_tokens_for_compaction =
        active_context_tokens_estimate.max(compaction_estimated_total_tokens);
    let token_status = auto_compact_token_status(AutoCompactPolicyInput {
        model_context_window: settings.model_context_window as usize,
        trigger_ratio: settings.context_compaction_trigger_ratio,
        configured_limit: settings.model_auto_compact_token_limit,
        scope: settings.model_auto_compact_token_limit_scope,
        active_context_tokens: active_context_tokens_for_compaction,
        window: auto_compact_window.snapshot(),
    });

    let compaction = maybe_compact_history_with_start_callback(
        host,
        context_manager.history_mut(),
        cancellation_token,
        CompactionMode::Automatic {
            _estimated_total_tokens: token_status.active_context_tokens,
            token_limit_reached: token_status.token_limit_reached,
            continuation: compaction_continuation(roundtrip_count),
        },
        |start| {
            emit_event(
                events,
                on_event,
                EventMsg::ContextCompactionStarted {
                    turn_id: turn_id.to_string(),
                    continuation: start.continuation,
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
                continuation: compacted.continuation,
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
                continuation: compacted.continuation,
                pre_context_tokens_estimate: compacted.pre_context_tokens_estimate,
                post_context_tokens_estimate: compacted.post_context_tokens_estimate,
                pre_message_count: compacted.pre_message_count,
                post_message_count: compacted.post_message_count,
                preserved_user_count: compacted.preserved_user_count,
            },
        );
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
    let injection_strategy = injection_strategy_for_compaction(
        compaction.as_ref().map(|compacted| compacted.continuation),
    );

    Ok(PreparedTurnContext {
        budgeted,
        prepared_fragments,
        injection_strategy,
        token_status,
        active_context_tokens_estimate,
        compaction,
    })
}

fn injection_strategy_for_compaction(
    continuation: Option<CompactionContinuation>,
) -> ContextInjectionStrategy {
    match continuation {
        Some(CompactionContinuation::MidTurn) => {
            ContextInjectionStrategy::MidTurnCompactionContinuation
        }
        _ => ContextInjectionStrategy::Standard,
    }
}
