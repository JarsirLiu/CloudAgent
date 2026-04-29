mod approval_overlay;
mod bottom_pane_view;
mod chat_composer;
mod console;
mod console_client;
mod console_events;
mod console_parse;
mod console_state;
mod console_status;
mod footer;
mod history_cell;
mod input_pane;
mod terminal_runtime;
mod textarea;
mod welcome;

pub use console::ConsoleConnection;
pub use console::{ConsoleConfig, run_console};
