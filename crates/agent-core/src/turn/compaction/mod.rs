mod context;
mod flow;
mod lifecycle;
mod manual;
mod model;
mod planner;
mod policy;
mod prepare;
mod service;
mod summarizer;
mod window;

#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod window_tests;

#[allow(unused_imports)]
pub(crate) use context::{BudgetedFragmentInputs, build_budgeted_fragments_for_current_history};
pub use flow::ManualCompactionOutcome;
#[allow(unused_imports)]
pub(crate) use flow::{
    AppliedCompaction, CompactionMode, maybe_compact_history,
    maybe_compact_history_with_start_callback,
};
pub use manual::run_manual_compaction;
pub use model::{
    CompactionOutcome, CompactionPhase, CompactionReason, CompactionRequest, CompactionTrigger,
    InitialContextInjection,
};
pub use policy::{
    AutoCompactPolicyInput, AutoCompactTokenLimitScope, AutoCompactTokenStatus,
    auto_compact_token_status,
};
#[allow(unused_imports)]
pub(crate) use prepare::{
    PreparedTurnContext, compaction_phase, prepare_turn_context_with_auto_compaction,
};
pub use window::{AutoCompactWindow, AutoCompactWindowSnapshot};
