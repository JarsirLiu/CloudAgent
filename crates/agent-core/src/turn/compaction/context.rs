use crate::context::{
    ContextBudgetSource, ContextFacade, ContextFragment, ContextManager, FilterPolicy,
    SkillBudgetSource,
};
use crate::skill::TurnSkillContext;

#[derive(Clone, Debug)]
pub(crate) struct BudgetedFragmentInputs {
    pub raw_memory_fragment: Option<String>,
    pub turn_skill_context: TurnSkillContext,
}

pub(crate) fn build_budgeted_fragments_for_current_history(
    context_facade: &ContextFacade,
    context_manager: &ContextManager,
    filter_policy: FilterPolicy,
    environment_context: &crate::context::EnvironmentContext,
    settings: &crate::turn::ChatTurnSettings,
    inputs: BudgetedFragmentInputs,
) -> crate::context::BudgetedFragments {
    context_facade.build_context_budgeted_fragments(
        &context_manager.history().messages,
        filter_policy,
        environment_context.render(),
        &settings.workspace_root,
        settings.model_context_window,
        settings.context_compaction_trigger_ratio,
        ContextBudgetSource {
            memory: inputs.raw_memory_fragment,
            skills: SkillBudgetSource {
                summary: inputs.turn_skill_context.catalog_summary,
                explicit_documents: inputs.turn_skill_context.explicit_documents,
                enable_summary_bucket: settings.enable_skill_bucket,
                post_compact_budget_tokens: settings.post_compact_skills_token_budget,
                max_tokens_per_item: settings.post_compact_max_tokens_per_skill,
            },
            mcp: None,
            enable_mcp_bucket: settings.enable_mcp_bucket,
            post_compact_budget_tokens: settings.post_compact_token_budget,
            post_compact_memory_floor_tokens: settings.post_compact_memory_floor_tokens,
            post_compact_mcp_budget_tokens: settings.post_compact_mcp_token_budget,
            post_compact_max_tokens_per_memory: settings.post_compact_max_tokens_per_memory,
            post_compact_max_tokens_per_mcp: settings.post_compact_max_tokens_per_mcp,
            safety_buffer_tokens: settings.context_budget_safety_buffer_tokens,
        },
    )
}
