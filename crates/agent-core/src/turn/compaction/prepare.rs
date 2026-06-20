use super::flow::AppliedCompaction;
use super::lifecycle::{AutoCompactionLifecycleInput, prepare_context_with_auto_compaction};
use super::policy::{AutoCompactPolicyInput, AutoCompactTokenStatus, auto_compact_token_status};
use super::window::AutoCompactWindow;
use crate::EventMsg;
use crate::context::{ContextFacade, ContextManager, FilterPolicy};
use crate::skill::TurnSkillContext;
use crate::turn::{RequestTokenBaseline, TurnHost, apply_signed_token_delta};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub(crate) struct PreparedTurnContext {
    pub budgeted: crate::context::BudgetedFragments,
    pub prepared_fragments: Vec<crate::ResponseItem>,
    pub token_status: AutoCompactTokenStatus,
    pub active_context_tokens_estimate: usize,
    pub compaction: Option<AppliedCompaction>,
}

pub(crate) fn compaction_phase(roundtrip_count: usize) -> crate::turn::CompactionPhase {
    if roundtrip_count <= 1 {
        crate::turn::CompactionPhase::PreTurn
    } else {
        crate::turn::CompactionPhase::MidTurn
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
    raw_memory_fragment: &Option<String>,
    turn_skill_context: &TurnSkillContext,
    request_baseline: &mut RequestTokenBaseline,
    auto_compact_window: &mut AutoCompactWindow,
    roundtrip_count: usize,
    events: &mut Vec<EventMsg>,
    on_event: &mut (dyn for<'a> FnMut(&'a EventMsg) + Send + '_),
) -> Result<PreparedTurnContext>
where
    H: TurnHost,
{
    let budgeted_before_compaction = super::context::build_budgeted_fragments_for_current_history(
        context_facade,
        context_manager,
        filter_policy,
        environment_context,
        settings,
        super::context::BudgetedFragmentInputs {
            raw_memory_fragment: raw_memory_fragment.clone(),
            turn_skill_context: turn_skill_context.clone(),
        },
    );
    let mut candidate_request = context_manager
        .build_current_model_request_with_rendered_fragments(
            &budgeted_before_compaction.fragments,
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
    prepare_context_with_auto_compaction(AutoCompactionLifecycleInput {
        host,
        conversation_id,
        turn_id,
        cancellation_token,
        context_facade,
        context_manager,
        filter_policy,
        environment_context,
        settings,
        raw_memory_fragment,
        turn_skill_context,
        request_baseline,
        auto_compact_window,
        token_status,
        active_context_tokens_estimate,
        roundtrip_count,
        events,
        on_event,
    })
    .await
}
