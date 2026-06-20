use crate::context::{
    CompactionSummary, ContextCompactionConfig, ContextFacade, build_compaction_summary_request,
    plan_manual_history_compaction,
};
use crate::conversation::ConversationHistory;
use crate::rollout::RolloutItem;
use crate::turn::TurnHost;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum ManualCompactionOutcome {
    Compacted {
        pre_context_tokens_estimate: u64,
        post_context_tokens_estimate: u64,
        pre_message_count: usize,
        post_message_count: usize,
        preserved_user_count: usize,
    },
    Skipped {
        estimated_history_tokens: usize,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct AppliedCompaction {
    pub summary: CompactionSummary,
    pub rendered_summary: String,
    pub replacement_history: Vec<crate::ResponseItem>,
    #[allow(dead_code)]
    pub continuation: CompactionContinuation,
    pub pre_context_tokens_estimate: u64,
    pub post_context_tokens_estimate: u64,
    pub pre_message_count: usize,
    pub post_message_count: usize,
    pub preserved_user_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CompactionStart {
    pub continuation: CompactionContinuation,
    pub estimated_history_tokens: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionContinuation {
    PreTurn,
    MidTurn,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CompactionMode {
    Manual {
        minimum_history_tokens: usize,
    },
    Automatic {
        _estimated_total_tokens: usize,
        token_limit_reached: bool,
        continuation: CompactionContinuation,
    },
}

pub(crate) async fn maybe_compact_history<H>(
    host: &H,
    history: &mut ConversationHistory,
    cancellation_token: &CancellationToken,
    mode: CompactionMode,
) -> Result<Option<AppliedCompaction>>
where
    H: TurnHost,
{
    maybe_compact_history_with_start_callback(host, history, cancellation_token, mode, |_| {}).await
}

pub(crate) async fn maybe_compact_history_with_start_callback<H, F>(
    host: &H,
    history: &mut ConversationHistory,
    cancellation_token: &CancellationToken,
    mode: CompactionMode,
    on_start: F,
) -> Result<Option<AppliedCompaction>>
where
    H: TurnHost,
    F: FnOnce(CompactionStart),
{
    let context_facade = ContextFacade::new();
    let settings = host.chat_turn_settings();
    let estimated_history_tokens = context_facade.estimate_history_tokens_for_canonical_compaction(
        &history.messages,
        &settings.workspace_root,
    );

    let compaction_config = ContextCompactionConfig {
        model_context_window: settings.model_context_window,
        trigger_ratio: settings.context_compaction_trigger_ratio,
        compacted_target_tokens: settings.context_compaction_target_tokens,
        preserved_user_turns: settings.context_compaction_preserved_user_turns,
        preserved_tail_tokens: settings.context_compaction_preserved_tail_tokens,
        summary_source_max_tokens: settings.context_compaction_summary_source_tokens,
    };

    let minimum_history_tokens = match mode {
        CompactionMode::Manual {
            minimum_history_tokens,
        } => {
            if estimated_history_tokens < minimum_history_tokens {
                return Ok(None);
            }
            minimum_history_tokens.max(1)
        }
        CompactionMode::Automatic {
            token_limit_reached,
            ..
        } => {
            if !token_limit_reached {
                return Ok(None);
            }
            1
        }
    };

    let filtered_messages =
        context_facade.filtered_messages_for_canonical_compaction(&history.messages);
    let Some(filtered_plan) = plan_manual_history_compaction(
        &filtered_messages,
        compaction_config,
        minimum_history_tokens,
    ) else {
        return Ok(None);
    };
    let Some(raw_plan) = plan_manual_history_compaction(
        &history.messages,
        compaction_config,
        minimum_history_tokens,
    ) else {
        return Ok(None);
    };
    let continuation = match mode {
        CompactionMode::Manual { .. } => CompactionContinuation::PreTurn,
        CompactionMode::Automatic { continuation, .. } => continuation,
    };
    on_start(CompactionStart {
        continuation,
        estimated_history_tokens,
    });

    let summary_request = build_compaction_summary_request(
        &filtered_plan,
        compaction_config,
        settings.llm_temperature,
    );
    let summary_response = host
        .complete_model_request(cancellation_token, summary_request)
        .await?;
    let summary = summary_response
        .content
        .map(|text| CompactionSummary::from_model_output(&text).ensure_defaults())
        .filter(|summary| !summary.current_task.is_empty())
        .unwrap_or_else(|| CompactionSummary::fallback_from_plan(&filtered_plan));

    let pre_message_count = history.messages.len();
    let pre_context_tokens_estimate =
        context_facade.estimate_history_tokens(&history.messages) as u64;
    let compacted = context_facade.apply_compaction(&mut history.messages, &raw_plan, summary);
    let post_message_count = compacted.replacement_history.len();
    let post_context_tokens_estimate =
        context_facade.estimate_history_tokens(&compacted.replacement_history) as u64;
    let preserved_user_count = compacted
        .replacement_history
        .iter()
        .filter(|item| {
            matches!(item, crate::ResponseItem::User { content } if {
                let text = crate::input_items_to_plain_text(content);
                let trimmed = text.trim_start();
                !trimmed.starts_with("[Context Summary]") && !trimmed.starts_with("<turn_aborted>")
            })
        })
        .count();
    let rendered_summary = compacted.summary.rendered();

    Ok(Some(AppliedCompaction {
        summary: compacted.summary,
        rendered_summary,
        replacement_history: compacted.replacement_history,
        continuation,
        pre_context_tokens_estimate,
        post_context_tokens_estimate,
        pre_message_count,
        post_message_count,
        preserved_user_count,
    }))
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
            continuation: compacted.continuation,
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
