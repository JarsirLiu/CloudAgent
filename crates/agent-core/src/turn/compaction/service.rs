use super::model::{CompactionOutcome, CompactionRequest};
use super::planner::plan_compaction;
use super::summarizer::summarize_compaction_plan;
use crate::conversation::ConversationHistory;
use crate::turn::TurnHost;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(crate) async fn run_compaction<H: TurnHost>(
    host: &H,
    history: &mut ConversationHistory,
    cancellation_token: &CancellationToken,
    request: &CompactionRequest,
) -> Result<Option<CompactionOutcome>> {
    let settings = host.chat_turn_settings();
    let Some(plan) = plan_compaction(history, &settings, request) else {
        return Ok(None);
    };

    let context_facade = crate::context::ContextFacade::new();
    let summary = summarize_compaction_plan(host, cancellation_token, &plan).await?;
    let pre_message_count = history.messages.len();
    let pre_context_tokens_estimate =
        context_facade.estimate_history_tokens(&history.messages) as u64;
    let compacted = context_facade.apply_compaction(&mut history.messages, &plan.raw_plan, summary);
    let post_message_count = compacted.replacement_history.len();
    let post_context_tokens_estimate =
        context_facade.estimate_history_tokens(&compacted.replacement_history) as u64;
    let preserved_user_count = compacted
        .replacement_history
        .iter()
        .filter(|item| crate::context::counts_as_real_user_turn(item))
        .count();
    let rendered_summary = compacted.summary.rendered();

    Ok(Some(CompactionOutcome {
        summary: compacted.summary,
        rendered_summary,
        replacement_history: compacted.replacement_history,
        trigger: request.trigger,
        reason: request.reason,
        phase: request.phase,
        pre_context_tokens_estimate,
        post_context_tokens_estimate,
        pre_message_count,
        post_message_count,
        preserved_user_count,
    }))
}
