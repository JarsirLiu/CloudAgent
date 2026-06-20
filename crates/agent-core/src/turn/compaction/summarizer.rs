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

    Ok(summary_response
        .content
        .map(|text| CompactionSummary::from_model_output(&text).ensure_defaults())
        .filter(|summary| !summary.current_task.is_empty())
        .unwrap_or_else(|| CompactionSummary::fallback_from_plan(&plan.filtered_plan)))
}
