mod regular;

use crate::AgentRuntime;
use agent_core::{
    CompactionSummary, ContextCompactionConfig, ContextFragment, ConversationHistory, RolloutItem,
    apply_history_compaction, build_compaction_summary_request, plan_manual_history_compaction,
};
use agent_protocol::{EventMsg, ServerRequest, ServerRequestDecision};
use anyhow::Result;
use tokio_util::sync::CancellationToken;

pub(crate) use regular::{
    RegularTurnTask, TurnOutcome, estimate_history_tokens, estimate_request_overhead_tokens,
};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskKind {
    Regular,
    Monitor,
    Wakeup,
}

pub(crate) struct TaskContext<'a, E> {
    pub(crate) runtime: &'a AgentRuntime,
    pub(crate) conversation_id: &'a str,
    pub(crate) turn_id: &'a str,
    pub(crate) cancellation_token: CancellationToken,
    pub(crate) on_event: &'a mut E,
}

pub(crate) trait RuntimeTask<E, F, Fut>
where
    E: FnMut(&EventMsg) + Send,
    F: Fn(ServerRequest) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<ServerRequestDecision>> + Send,
{
    #[allow(dead_code)]
    fn kind(&self) -> TaskKind;

    fn run(
        self,
        ctx: TaskContext<'_, E>,
        history: ConversationHistory,
        approval: F,
    ) -> impl std::future::Future<Output = Result<TurnOutcome>> + Send;
}

pub(crate) async fn run_manual_compaction(
    runtime: &AgentRuntime,
    conversation_id: &str,
    minimum_history_tokens: usize,
) -> Result<crate::ManualCompactionOutcome> {
    let mut history = runtime.load_history(conversation_id).await?;
    let tool_specs = runtime.tools.specs();
    let environment_context = runtime.environment_context();
    let estimated_history_tokens = estimate_history_tokens(&history.messages);
    if estimated_history_tokens < minimum_history_tokens {
        return Ok(crate::ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        });
    }

    let compaction_config = ContextCompactionConfig {
        model_context_window: runtime.config.runtime.model_context_window,
        trigger_ratio: runtime.config.runtime.context_compaction_trigger_ratio,
        request_overhead_tokens: estimate_request_overhead_tokens(
            &history.messages,
            &environment_context.render(),
            &tool_specs,
            runtime
                .config
                .runtime
                .context_compaction_request_overhead_tokens,
        ),
        compacted_target_tokens: runtime.config.runtime.context_compaction_target_tokens,
        preserved_user_turns: runtime
            .config
            .runtime
            .context_compaction_preserved_user_turns,
        preserved_tail_tokens: runtime
            .config
            .runtime
            .context_compaction_preserved_tail_tokens,
        summary_source_max_tokens: runtime
            .config
            .runtime
            .context_compaction_summary_source_tokens,
    };

    let Some(compaction_plan) = plan_manual_history_compaction(
        &history.messages,
        compaction_config,
        minimum_history_tokens,
    ) else {
        return Ok(crate::ManualCompactionOutcome::Skipped {
            estimated_history_tokens,
        });
    };

    let summary_request = build_compaction_summary_request(
        &compaction_plan,
        compaction_config,
        runtime.config.llm.temperature,
    );
    let summary_response = runtime.model.complete(summary_request).await?;
    let summary = summary_response
        .content
        .map(|text| CompactionSummary::from_model_output(&text).ensure_defaults())
        .filter(|summary| !summary.current_task.is_empty())
        .unwrap_or_else(|| CompactionSummary::fallback_from_plan(&compaction_plan));

    let pre_message_count = history.messages.len();
    let pre_context_tokens_estimate = estimate_history_tokens(&history.messages) as u64;
    let compacted = apply_history_compaction(&mut history.messages, &compaction_plan, summary);
    let post_message_count = compacted.replacement_history.len();
    let post_context_tokens_estimate =
        estimate_history_tokens(&compacted.replacement_history) as u64;
    let preserved_tail_count = post_message_count.saturating_sub(2);
    let rendered_summary = compacted.summary.rendered();

    runtime
        .persist_rollout_items(
            conversation_id,
            &[RolloutItem::Compacted {
                summary: compacted.summary,
                rendered_summary,
                replacement_history: compacted.replacement_history.clone(),
            }],
        )
        .await?;
    runtime.save_history(history).await?;
    runtime.rollout_recorder.flush().await?;

    Ok(crate::ManualCompactionOutcome::Compacted {
        pre_context_tokens_estimate,
        post_context_tokens_estimate,
        pre_message_count,
        post_message_count,
        preserved_tail_count,
    })
}
