pub mod actions;
mod app_lifecycle;
mod conversation_facade;
mod core;
mod event_router;
pub mod effects;
mod facade;
mod filter_toggle;
mod items;
mod parse;
mod runtime_loop;
mod runtime_updates;

pub use crate::app::core::types::{ConsoleConfig, ConsoleConnection};
pub use crate::app::facade::run_console;
pub(crate) use crate::app::core::types::TuiApp;

#[cfg(test)]
mod tests;
