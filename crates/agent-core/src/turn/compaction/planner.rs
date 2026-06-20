use super::CompactionRequest;
use crate::context::{
    ContextCompactionConfig, ContextCompactionPlan, ContextFacade, plan_manual_history_compaction,
};
use crate::conversation::ConversationHistory;
use crate::turn::ChatTurnSettings;

#[derive(Clone, Debug)]
pub(crate) struct PlannedCompaction {
    pub config: ContextCompactionConfig,
    pub filtered_plan: ContextCompactionPlan,
    pub raw_plan: ContextCompactionPlan,
}

pub(crate) fn plan_compaction(
    history: &ConversationHistory,
    settings: &ChatTurnSettings,
    request: &CompactionRequest,
) -> Option<PlannedCompaction> {
    let context_facade = ContextFacade::new();
    let estimated_history_tokens = context_facade.estimate_history_tokens_for_canonical_compaction(
        &history.messages,
        &settings.workspace_root,
    );

    let config = ContextCompactionConfig {
        model_context_window: settings.model_context_window,
        trigger_ratio: settings.context_compaction_trigger_ratio,
        compacted_target_tokens: settings.context_compaction_target_tokens,
        preserved_user_turns: settings.context_compaction_preserved_user_turns,
        preserved_tail_tokens: settings.context_compaction_preserved_tail_tokens,
        summary_source_max_tokens: settings.context_compaction_summary_source_tokens,
    };

    let minimum_history_tokens = request.minimum_history_tokens.max(1);
    if estimated_history_tokens < minimum_history_tokens {
        return None;
    }

    let filtered_messages =
        context_facade.filtered_messages_for_canonical_compaction(&history.messages);
    let filtered_plan =
        plan_manual_history_compaction(&filtered_messages, config, minimum_history_tokens)?;
    let raw_plan =
        plan_manual_history_compaction(&history.messages, config, minimum_history_tokens)?;

    Some(PlannedCompaction {
        config,
        filtered_plan,
        raw_plan,
    })
}
