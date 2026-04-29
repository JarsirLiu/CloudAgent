mod history;
mod turn_output;

pub use history::history_entry_from_message;
pub use turn_output::{agent_turn_output_from_events, tool_events_from_turn_events};

pub fn module_name() -> &'static str {
    "agent-core::projection"
}
