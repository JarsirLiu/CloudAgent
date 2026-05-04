use crate::context::{
    CompactionSummary, ContextCompactionConfig, ContextFacade, apply_history_compaction,
    build_compaction_summary_request, plan_manual_history_compaction, ContextFragment,
};
use crate::rollout::RolloutItem;
use crate::turn::TurnHost;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum ManualCompactionOutcome {
    Compacted {
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_tail_count: usize,
    },
    Skipped {
        estimated_history_tokens: usize,
    },
}

pub async fn run_manual_compaction<H>(
    host: &H,
    conversation_id: &str,
    minimum_history_tokens: usize,
) -> Result<ManualCompactionOutcome>
where
    H: TurnHost,
{
    let mut history = host.load_history(conversation_id).await?;
    let context_facade = ContextFacade::new();
    let tool_specs = host.all_tool_specs();
    let environment_context = host.environment_context();
    let settings = host.regular_turn_settings();
    let estimated_history_tokens = context_facade.estimate_history_tokens_for_canonical_compaction(
        &history.messages,
        &settings.workspace_root,
    );
    if estimated_history_tokens < minimum_history_tokens {
        return Ok(ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        });
    }

    let compaction_config = ContextCompactionConfig {
        model_context_window: settings.model_context_window,
        trigger_ratio: settings.context_compaction_trigger_ratio,
        request_overhead_tokens: context_facade.estimate_request_overhead_tokens(
            &history.messages,
            &environment_context.render(),
            &tool_specs,
            settings.context_compaction_request_overhead_tokens,
        ),
        compacted_target_tokens: settings.context_compaction_target_tokens,
        preserved_user_turns: settings.context_compaction_preserved_user_turns,
        preserved_tail_tokens: settings.context_compaction_preserved_tail_tokens,
        summary_source_max_tokens: settings.context_compaction_summary_source_tokens,
    };

    let filtered_messages =
        context_facade.filtered_messages_for_canonical_compaction(&history.messages);
    let Some(filtered_plan) = plan_manual_history_compaction(
        &filtered_messages,
        compaction_config,
        minimum_history_tokens,
    ) else {
        return Ok(ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        });
    };
    let Some(raw_plan) =
        plan_manual_history_compaction(&history.messages, compaction_config, minimum_history_tokens)
    else {
        return Ok(ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        });
    };

    let summary_request = build_compaction_summary_request(
        &filtered_plan,
        compaction_config,
        settings.llm_temperature,
    );
    let summary_response = host
        .complete_model_request(&CancellationToken::new(), summary_request)
        .await?;
    let summary = summary_response
        .content
        .map(|text| CompactionSummary::from_model_output(&text).ensure_defaults())
        .filter(|summary| !summary.current_task.is_empty())
        .unwrap_or_else(|| CompactionSummary::fallback_from_plan(&filtered_plan));

    let pre_message_count = history.messages.len();
    let pre_context_tokens_estimate = context_facade.estimate_history_tokens(&history.messages) as u64;
    let compacted = apply_history_compaction(&mut history.messages, &raw_plan, summary);
    let post_message_count = compacted.replacement_history.len();
    let post_context_tokens_estimate =
        context_facade.estimate_history_tokens(&compacted.replacement_history) as u64;
    let preserved_tail_count = post_message_count.saturating_sub(2);
    let rendered_summary = compacted.summary.rendered();

    host.persist_rollout_items(
        conversation_id,
        &[RolloutItem::Compacted {
            summary: compacted.summary,
            rendered_summary,
            replacement_history: compacted.replacement_history.clone(),
        }],
    )
    .await?;
    host.save_history(history).await?;
    host.flush_rollout().await?;

    Ok(ManualCompactionOutcome::Compacted {
        pre_context_tokens_estimate,
        post_context_tokens_estimate,
        pre_message_count,
        post_message_count,
        preserved_tail_count,
    })
}
