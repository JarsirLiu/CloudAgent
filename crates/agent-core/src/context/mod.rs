mod compaction;
mod environment;
mod fragments;
mod manager;
mod tool_context;

pub use compaction::{
    CompactionSummary, ContextCompactionConfig, ContextCompactionPlan, ContextCompactionResult,
    apply_history_compaction, build_compaction_summary_request, plan_history_compaction,
    plan_manual_history_compaction,
};
pub use environment::EnvironmentContext;
pub use fragments::ContextFragment;
pub use manager::{ContextManager, ModelContext};
pub use tool_context::{AgentContext, ToolExecutionContext};
