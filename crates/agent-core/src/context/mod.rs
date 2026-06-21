mod budget;
mod compaction;
mod environment;
mod facade;
mod fragments;
mod input_filter;
mod manager;
mod markers;
mod tool_context;

#[cfg(test)]
mod budget_tests;

#[cfg(test)]
mod fragments_tests;

pub use budget::{
    BucketAudit, BudgetedFragments, ContextBudgetSource, SkillBucketAudit, SkillBudgetSource,
    build_context_budgeted_fragments,
};
pub use compaction::{
    CompactedReplacementHistory, CompactionSummary, ContextCompactionConfig, ContextCompactionPlan,
    ContextCompactionResult, apply_history_compaction, build_compacted_replacement_history,
    build_compaction_summary_request, plan_history_compaction, plan_manual_history_compaction,
};
pub use environment::EnvironmentContext;
pub use facade::ContextFacade;
pub use fragments::ContextFragment;
pub use input_filter::{ContextInputFilterService, FilterPolicy};
pub use manager::{ContextManager, ModelContext};
pub use markers::{
    append_turn_aborted_marker_if_needed, context_summary_prefix, counts_as_real_user_turn,
    is_context_summary_item, is_turn_aborted_marker, turn_aborted_marker_item,
    turn_aborted_marker_text,
};
pub use tool_context::{AgentContext, ToolExecutionContext};
