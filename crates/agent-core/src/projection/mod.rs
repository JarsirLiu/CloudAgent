mod history;

pub use history::history_entry_from_message;

pub fn module_name() -> &'static str {
    "agent-core::projection"
}
