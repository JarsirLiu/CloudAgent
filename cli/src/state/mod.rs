//! Application state, reducers, and state-machine helpers for the CLI app.
//!
//! This module owns the mutable runtime state that is shared across app layers:
//! - `RunState` and `WeixinBindingState` model the current conversation/runtime state
//! - `reducer` translates server messages into state actions
//! - `bottom_pane_*` modules derive UI state for the input pane and status area
//! - `selectors` exposes read-only state helpers used by rendering and input routing
//! - `turn_lifecycle` tracks the lifecycle of the active conversation turn

pub mod bottom_pane_controller;
pub mod bottom_pane_runtime;
pub mod notification;
pub mod notification_store;
pub mod reducer;
mod reducer_routes;
pub mod selectors;
pub mod turn_lifecycle;

use agent_core::{ConversationTurn, ModelUsage};
use agent_protocol::ConversationViewSnapshot;
use std::time::Instant;
use turn_lifecycle::TurnLifecycleState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct RunState {
    pub(crate) history_snapshot: Option<Vec<ConversationTurn>>,
    pub(crate) conversation_view_snapshot: Option<ConversationViewSnapshot>,
    pub(crate) history_has_more: bool,
    pub(crate) history_next_before_turn_id: Option<String>,
    pub(crate) last_turn_usage: Option<ModelUsage>,
    pub(crate) total_turn_usage: Option<ModelUsage>,
    pub(crate) model_context_window: Option<u64>,
    pub(crate) turn_lifecycle: TurnLifecycleState,
    pub(crate) should_exit: bool,
    pub(crate) live_animation_frame: u64,
    pub(crate) expand_tool_details: bool,
    pub(crate) pre_llm_filter_enabled: bool,
    pub(crate) permission_mode: String,
    pub(crate) weixin_binding: Option<WeixinBindingState>,
    pub(crate) pending_skills_refresh: bool,
    pub(crate) next_skills_refresh_at: Option<Instant>,
    pub(crate) seen_model_catalog_version: u64,
}

#[derive(Clone, Debug)]
pub struct WeixinBindingState {
    pub platform: String,
    pub session_id: String,
    pub qr_url: String,
    pub status: String,
    pub next_poll_at: Instant,
}

impl RunState {
    pub fn new(_connection_label: &str) -> Self {
        Self {
            history_snapshot: None,
            conversation_view_snapshot: None,
            history_has_more: false,
            history_next_before_turn_id: None,
            last_turn_usage: None,
            total_turn_usage: None,
            model_context_window: None,
            turn_lifecycle: TurnLifecycleState::new(),
            should_exit: false,
            live_animation_frame: 0,
            expand_tool_details: false,
            pre_llm_filter_enabled: false,
            permission_mode: "WorkspaceWrite".to_string(),
            weixin_binding: None,
            pending_skills_refresh: false,
            next_skills_refresh_at: None,
            seen_model_catalog_version: 0,
        }
    }
}
