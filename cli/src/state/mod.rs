pub mod reducer;
pub mod runtime_projection;
pub mod selectors;
pub mod status_view_model;

use crate::ui::widgets::history_cell::{HistoryCell, Transcript};
use agent_protocol::{ConversationTurn, FrontendMode, ModelUsage, RequestId};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ConsoleState {
    pub mode: FrontendMode,
}

impl ConsoleState {
    pub fn new() -> Self {
        Self {
            mode: FrontendMode::Idle,
        }
    }

    pub fn can_submit_turn(&self) -> bool {
        self.mode == FrontendMode::Idle
    }
}

#[derive(Clone, Debug, Default)]
pub struct ServerRequestState {
    pub active_request_id: Option<RequestId>,
    pub action_required: bool,
}

#[derive(Default)]
pub struct TranscriptState {
    pub transcript: Transcript,
    pub active_item_id: Option<String>,
    pub active_item_kind: Option<agent_protocol::TurnItemKind>,
    pub active_cell: Option<HistoryCell>,
    pub last_copyable_output: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SystemNotice {
    pub text: String,
    pub expires_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct RunState {
    pub history_loaded: bool,
    pub history_snapshot: Option<Vec<ConversationTurn>>,
    pub system_notice: Option<SystemNotice>,
    pub last_turn_usage: Option<ModelUsage>,
    pub total_turn_usage: Option<ModelUsage>,
    pub model_context_window: Option<u64>,
    pub should_exit: bool,
    pub expand_tool_details: bool,
    pub pre_llm_filter_enabled: bool,
    pub permission_mode: String,
}

impl RunState {
    pub fn new(connection_label: &str) -> Self {
        Self {
            history_loaded: false,
            history_snapshot: None,
            system_notice: Some(SystemNotice {
                text: format!("Connected via {connection_label}"),
                expires_at: None,
            }),
            last_turn_usage: None,
            total_turn_usage: None,
            model_context_window: None,
            should_exit: false,
            expand_tool_details: false,
            pre_llm_filter_enabled: false,
            permission_mode: "safe".to_string(),
        }
    }
}

impl RunState {
    fn default_ttl(level: NoticeLevel) -> Duration {
        match level {
            NoticeLevel::Info => Duration::from_secs(3),
            NoticeLevel::Warn => Duration::from_secs(5),
            NoticeLevel::Error => Duration::from_secs(8),
        }
    }

    pub fn set_system_notice(&mut self, text: impl Into<String>, ttl: Option<Duration>) {
        self.system_notice = Some(SystemNotice {
            text: text.into(),
            expires_at: ttl.map(|d| Instant::now() + d),
        });
    }

    pub fn set_system_notice_level(&mut self, text: impl Into<String>, level: NoticeLevel) {
        self.set_system_notice(text, Some(Self::default_ttl(level)));
    }

    pub fn clear_system_notice(&mut self) {
        self.system_notice = None;
    }

    pub fn current_system_notice(&self) -> Option<&str> {
        self.system_notice
            .as_ref()
            .and_then(|n| match n.expires_at {
                Some(t) if Instant::now() > t => None,
                _ => Some(n.text.as_str()),
            })
    }

    pub fn clear_expired_notices(&mut self) {
        if self
            .system_notice
            .as_ref()
            .and_then(|n| n.expires_at)
            .is_some_and(|t| Instant::now() > t)
        {
            self.system_notice = None;
        }
    }
}
