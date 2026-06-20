use super::model::{
    CompactionOutcome, CompactionPhase, CompactionReason, CompactionRequest, CompactionTrigger,
};
use super::service::run_compaction;
use crate::context::{ContextFacade, counts_as_real_user_turn};
use crate::conversation::ConversationHistory;
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
        preserved_user_count: usize,
    },
    Skipped {
        estimated_history_tokens: usize,
    },
}

pub(crate) type AppliedCompaction = CompactionOutcome;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CompactionStart {
    pub trigger: CompactionTrigger,
    pub reason: CompactionReason,
    pub phase: CompactionPhase,
    pub estimated_history_tokens: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CompactionMode {
    Manual {
        minimum_history_tokens: usize,
    },
    Automatic {
        estimated_total_tokens: usize,
        token_limit_reached: bool,
        phase: CompactionPhase,
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
    let (trigger, reason, phase, minimum_history_tokens) = match mode {
        CompactionMode::Manual {
            minimum_history_tokens,
        } => (
            CompactionTrigger::Manual,
            CompactionReason::UserRequested,
            CompactionPhase::StandaloneTurn,
            minimum_history_tokens.max(1),
        ),
        CompactionMode::Automatic {
            token_limit_reached,
            phase,
            ..
        } => {
            if !token_limit_reached {
                return Ok(None);
            }
            (
                CompactionTrigger::Auto,
                CompactionReason::ContextLimit,
                phase,
                1,
            )
        }
    };
    let context_facade = ContextFacade::new();
    let settings = host.chat_turn_settings();
    let estimated_history_tokens = context_facade.estimate_history_tokens_for_canonical_compaction(
        &history.messages,
        &settings.workspace_root,
    );
    on_start(CompactionStart {
        trigger,
        reason,
        phase,
        estimated_history_tokens,
    });
    let request = CompactionRequest {
        conversation_id: String::new(),
        turn_id: String::new(),
        trigger,
        reason,
        phase,
        estimated_total_tokens: match mode {
            CompactionMode::Automatic {
                estimated_total_tokens,
                ..
            } => Some(estimated_total_tokens),
            CompactionMode::Manual { .. } => None,
        },
        minimum_history_tokens,
    };
    let Some(compacted) = run_compaction(host, history, cancellation_token, &request).await? else {
        return Ok(None);
    };
    debug_assert_eq!(compacted.phase, phase);
    debug_assert_eq!(
        compacted.preserved_user_count,
        compacted
            .replacement_history
            .iter()
            .filter(|item| counts_as_real_user_turn(item))
            .count()
    );
    Ok(Some(compacted))
}
