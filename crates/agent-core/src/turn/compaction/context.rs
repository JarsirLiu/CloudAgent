use crate::context::{
    ContextFacade, ContextFragment, ContextManager, FilterPolicy, MemoryBudgetSource,
};

#[derive(Clone, Debug)]
pub(crate) struct BudgetedFragmentInputs {
    pub raw_memory_fragment: Option<String>,
    pub skill_summary: Option<String>,
}

pub(crate) fn build_budgeted_fragments_for_current_history(
    context_facade: &ContextFacade,
    context_manager: &ContextManager,
    filter_policy: FilterPolicy,
    environment_context: &crate::context::EnvironmentContext,
    settings: &crate::turn::ChatTurnSettings,
    inputs: BudgetedFragmentInputs,
) -> crate::context::BudgetedFragments {
    context_facade.build_memory_budgeted_fragments(
        &context_manager.history().messages,
        filter_policy,
        environment_context.render(),
        &settings.workspace_root,
        settings.model_context_window,
        settings.context_compaction_trigger_ratio,
        MemoryBudgetSource {
            memory: inputs.raw_memory_fragment,
            skills: inputs.skill_summary,
            mcp: None,
            enable_skills_bucket: settings.enable_skill_bucket,
            enable_mcp_bucket: settings.enable_mcp_bucket,
            post_compact_budget_tokens: settings.post_compact_token_budget,
            post_compact_memory_floor_tokens: settings.post_compact_memory_floor_tokens,
            post_compact_skills_budget_tokens: settings.post_compact_skills_token_budget,
            post_compact_mcp_budget_tokens: settings.post_compact_mcp_token_budget,
            post_compact_max_tokens_per_memory: settings.post_compact_max_tokens_per_memory,
            post_compact_max_tokens_per_skill: settings.post_compact_max_tokens_per_skill,
            post_compact_max_tokens_per_mcp: settings.post_compact_max_tokens_per_mcp,
            safety_buffer_tokens: settings.context_budget_safety_buffer_tokens,
        },
    )
}

pub(crate) fn append_rendered_fragments(
    mut fragments: Vec<crate::ResponseItem>,
    extra_fragments: &[crate::ResponseItem],
) -> Vec<crate::ResponseItem> {
    fragments.extend(extra_fragments.iter().cloned());
    fragments
}
