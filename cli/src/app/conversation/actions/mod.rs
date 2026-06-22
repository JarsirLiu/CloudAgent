//! Conversation action handling.
//!
//! This module owns the command/action entrypoint for the conversation domain:
//! - local input dispatch for slash commands and picker flows
//! - server action reduction for conversation state updates
//! - history page loading and related bookkeeping
//!
//! The implementation is grouped by business responsibility inside this directory
//! instead of being flattened back into the parent `conversation` module.

use crate::app::TuiApp;
use crate::state::NoticeLevel;
use std::fmt::Display;

#[path = "local_actions.rs"]
mod local_actions;
#[path = "server_actions.rs"]
mod server_actions;

pub(crate) use local_actions::handle_tui_input;
pub(crate) use server_actions::execute_server_action;
pub(crate) use server_actions::load_older_history_page_if_available;
#[cfg(test)]
pub(crate) use server_actions::prepend_turn_page;

pub(crate) fn show_local_notice(app: &mut TuiApp, level: NoticeLevel, message: impl Into<String>) {
    app.bottom_pane.push_toast(level, message.into());
}

pub(crate) fn platform_request_notice(action: &str, err: &impl Display) -> String {
    let detail = err.to_string();
    if detail.contains("unsupported request method: platform/") {
        return format!(
            "Platform management is unavailable on the connected node while trying to {action}. \
Restart the local node with the latest build, then try /gateway again."
        );
    }
    format!("Failed to {action}: {detail}")
}

pub(crate) fn decision_label(decision: &agent_core::ServerRequestDecisionKind) -> &'static str {
    match decision {
        agent_core::ServerRequestDecisionKind::Accept => "approved",
        agent_core::ServerRequestDecisionKind::AcceptForSession => "approved for session",
        agent_core::ServerRequestDecisionKind::Decline => "denied",
        agent_core::ServerRequestDecisionKind::Cancel => "cancelled",
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
