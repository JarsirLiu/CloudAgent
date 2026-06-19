pub(crate) mod commands;
pub(crate) mod config;
pub(crate) mod conversation;
mod core;
pub(crate) mod input;
pub mod effects;
mod facade;
pub(crate) mod model_catalog;
pub(crate) mod runtime;

pub(crate) use crate::app::core::types::TuiApp;
pub use crate::app::core::types::{AppServerTarget, ConsoleBootstrap, ConsoleConfig};
pub use crate::app::facade::run_console;

#[cfg(test)]
mod tests;
