mod context;
mod flow;
mod policy;
mod prepare;
mod window;

#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod window_tests;

#[allow(unused_imports)]
pub(crate) use context::{
    BudgetedFragmentInputs, append_rendered_fragments, build_budgeted_fragments_for_current_history,
};
#[allow(unused_imports)]
pub(crate) use flow::{
    AppliedCompaction, CompactionMode, maybe_compact_history,
    maybe_compact_history_with_start_callback,
};
pub use flow::{CompactionContinuation, ManualCompactionOutcome, run_manual_compaction};
pub use policy::{
    AutoCompactPolicyInput, AutoCompactTokenLimitScope, AutoCompactTokenStatus,
    auto_compact_token_status,
};
#[allow(unused_imports)]
pub(crate) use prepare::{
    PreparedTurnContext, compaction_continuation, prepare_turn_context_with_auto_compaction,
};
pub use window::{AutoCompactWindow, AutoCompactWindowSnapshot};
