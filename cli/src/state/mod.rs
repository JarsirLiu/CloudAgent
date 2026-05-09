pub mod bottom_pane_controller;
pub mod bottom_pane_runtime;
pub mod reducer;
pub mod selectors;

use agent_core::ConversationTurn;
use agent_core::InputItem;
use agent_core::ModelUsage;
use agent_protocol::FrontendMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct RunState {
    pub history_snapshot: Option<Vec<ConversationTurn>>,
    pub last_turn_usage: Option<ModelUsage>,
    pub total_turn_usage: Option<ModelUsage>,
    pub model_context_window: Option<u64>,
    pub pending_submitted_input: Option<Vec<InputItem>>,
    pub frontend_mode: FrontendMode,
    pub should_exit: bool,
    pub live_animation_frame: u64,
    pub expand_tool_details: bool,
    pub pre_llm_filter_enabled: bool,
    pub permission_mode: String,
}

impl RunState {
    pub fn new(_connection_label: &str) -> Self {
        Self {
            history_snapshot: None,
            last_turn_usage: None,
            total_turn_usage: None,
            model_context_window: None,
            pending_submitted_input: None,
            frontend_mode: FrontendMode::Idle,
            should_exit: false,
            live_animation_frame: 0,
            expand_tool_details: false,
            pre_llm_filter_enabled: false,
            permission_mode: "ReadOnly".to_string(),
        }
    }
}
