use super::planner::PlannedCompaction;
use crate::context::{CompactionSummary, build_compaction_summary_request};
use crate::turn::TurnHost;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(crate) async fn summarize_compaction_plan<H: TurnHost>(
    host: &H,
    cancellation_token: &CancellationToken,
    plan: &PlannedCompaction,
) -> Result<CompactionSummary> {
    let settings = host.chat_turn_settings();
    let summary_request = build_compaction_summary_request(
        &plan.filtered_plan,
        plan.config,
        settings.llm_temperature,
    );
    let summary_response = host
        .complete_model_request(cancellation_token, summary_request)
        .await?;

    let Some(text) = summary_response.content else {
        anyhow::bail!("compaction summary model returned no content");
    };
    let summary = CompactionSummary::from_model_output(&text);
    if summary.current_task.is_empty()
        && summary.progress.is_empty()
        && summary.key_decisions.is_empty()
        && summary.important_context.is_empty()
        && summary.tool_code_facts.is_empty()
        && summary.next_steps.is_empty()
    {
        anyhow::bail!("compaction summary model returned an empty structured summary");
    }
    Ok(summary)
}
