pub mod cli_settings;
pub(crate) mod commands;
pub(crate) mod conversation;
mod core;
pub mod effects;
mod facade;
pub(crate) mod runtime;

pub(crate) use crate::app::core::types::TuiApp;
pub use crate::app::core::types::{ConsoleConfig, ConsoleConnection};
pub use crate::app::facade::run_console;

#[cfg(test)]
mod tests;
