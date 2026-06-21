pub mod agent_host;
pub mod app;
pub mod console_entry;
pub mod input;
pub mod local_node;
pub mod state;
pub mod terminal;
mod text_width;
mod tool_identity;
pub mod transport;
pub mod ui;

pub use app::{AppServerTarget, ConsoleBootstrap, ConsoleConfig, run_console};
