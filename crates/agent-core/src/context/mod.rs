mod budget;
mod compaction;
mod environment;
mod facade;
mod fragments;
mod input_filter;
mod manager;
mod tool_context;

#[cfg(test)]
mod fragments_tests;

pub use budget::{
    BucketAudit, BudgetedFragments, MemoryBudgetSource, build_memory_budgeted_fragments,
};
pub use compaction::{
    CompactedReplacementHistory, CompactionSummary, ContextCompactionConfig, ContextCompactionPlan,
    ContextCompactionResult, apply_history_compaction, build_compacted_replacement_history,
    build_compaction_summary_request, plan_history_compaction, plan_manual_history_compaction,
};
pub use environment::EnvironmentContext;
pub use facade::ContextFacade;
pub use fragments::{ContextFragment, ContextInjectionStrategy};
pub use input_filter::{ContextInputFilterService, FilterPolicy};
pub use manager::{ContextManager, ModelContext};
pub use tool_context::{AgentContext, ToolExecutionContext};
