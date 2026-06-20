use super::flow::ManualCompactionOutcome;
use super::flow::{CompactionMode, maybe_compact_history};
use crate::context::ContextFacade;
use crate::rollout::RolloutItem;
use crate::turn::TurnHost;
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub async fn run_manual_compaction<H>(
    host: &H,
    conversation_id: &str,
    minimum_history_tokens: usize,
) -> Result<ManualCompactionOutcome>
where
    H: TurnHost,
{
    let mut history = host.load_history(conversation_id).await?;
    let Some(compacted) = maybe_compact_history(
        host,
        &mut history,
        &CancellationToken::new(),
        CompactionMode::Manual {
            minimum_history_tokens,
        },
    )
    .await?
    else {
        let context_facade = ContextFacade::new();
        let estimated_history_tokens = context_facade
            .estimate_history_tokens_for_canonical_compaction(
                &history.messages,
                &host.chat_turn_settings().workspace_root,
            );
        return Ok(ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        });
    };

    host.persist_rollout_items(
        conversation_id,
        &[RolloutItem::Compacted {
            summary: compacted.summary,
            rendered_summary: compacted.rendered_summary,
            trigger: compacted.trigger,
            reason: compacted.reason,
            phase: compacted.phase,
            replacement_history: compacted.replacement_history,
        }],
    )
    .await?;
    host.save_history(history).await?;
    host.flush_rollout().await?;

    Ok(ManualCompactionOutcome::Compacted {
        pre_context_tokens_estimate: compacted.pre_context_tokens_estimate,
        post_context_tokens_estimate: compacted.post_context_tokens_estimate,
        pre_message_count: compacted.pre_message_count,
        post_message_count: compacted.post_message_count,
        preserved_user_count: compacted.preserved_user_count,
    })
}
