pub(crate) mod commands;
mod conversation;
mod core;
pub mod effects;
mod facade;
mod runtime;

pub use crate::app::core::types::{ConsoleConfig, ConsoleConnection};
pub use crate::app::facade::run_console;
pub(crate) use crate::app::core::types::TuiApp;

#[cfg(test)]
mod tests;
