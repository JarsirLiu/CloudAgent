pub(crate) mod active_runtime;
pub mod agent_host;
pub mod app;
pub mod console_entry;
pub mod input;
pub mod local_node;
mod runtime_metrics_display;
pub mod state;
pub mod terminal;
mod text_width;
mod tool_identity;
pub mod transport;
pub mod ui;

#[cfg(test)]
mod active_runtime_tests;

pub use app::{AppServerTarget, ConsoleBootstrap, ConsoleConfig, run_console};
