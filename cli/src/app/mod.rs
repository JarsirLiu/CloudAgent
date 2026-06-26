pub(crate) mod commands;
pub(crate) mod config;
pub(crate) mod conversation;
mod core;
pub mod effects;
mod facade;
pub(crate) mod input;
pub(crate) mod model_catalog;
pub(crate) mod runtime;
mod session;

pub use crate::app::config::cli_settings::{
    PersistedCliSettings, load_cli_settings, save_cli_settings,
};
pub(crate) use crate::app::core::types::TuiApp;
pub use crate::app::core::types::{AppServerTarget, ConsoleBootstrap, ConsoleConfig};
pub use crate::app::facade::run_console;

#[cfg(test)]
mod tests;
