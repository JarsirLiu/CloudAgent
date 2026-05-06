pub mod reducer;
pub mod runtime_projection;
pub mod selectors;
pub mod status_view_model;

use crate::ui::widgets::history_cell::Transcript;
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

impl Default for ConsoleState {
    fn default() -> Self {
        Self::new()
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
    pub last_copyable_output: Option<String>,
    pub scroll_offset_lines: usize,
    pub last_total_lines: usize,
    pub last_viewport_height: usize,
}

impl TranscriptState {
    pub fn reset_scroll(&mut self) {
        self.scroll_offset_lines = 0;
        self.last_total_lines = 0;
        self.last_viewport_height = 0;
    }

    pub fn note_total_lines(&mut self, total_lines: usize) {
        if self.scroll_offset_lines > 0 {
            if total_lines >= self.last_total_lines {
                self.scroll_offset_lines = self
                    .scroll_offset_lines
                    .saturating_add(total_lines - self.last_total_lines);
            } else {
                self.scroll_offset_lines = self
                    .scroll_offset_lines
                    .saturating_sub(self.last_total_lines - total_lines);
            }
        }
        self.last_total_lines = total_lines;
    }

    pub fn set_viewport_height(&mut self, viewport_height: usize) {
        self.last_viewport_height = viewport_height;
    }

    pub fn clamp_scroll(&mut self) {
        let max_offset = self
            .last_total_lines
            .saturating_sub(self.last_viewport_height);
        self.scroll_offset_lines = self.scroll_offset_lines.min(max_offset);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let max_offset = self
            .last_total_lines
            .saturating_sub(self.last_viewport_height);
        self.scroll_offset_lines = self.scroll_offset_lines.saturating_add(lines).min(max_offset);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset_lines = self.scroll_offset_lines.saturating_sub(lines);
    }

    pub fn page_up(&mut self) {
        self.scroll_up(self.last_viewport_height.max(1));
    }

    pub fn page_down(&mut self) {
        self.scroll_down(self.last_viewport_height.max(1));
    }

    pub fn jump_to_top(&mut self) {
        self.scroll_offset_lines = self
            .last_total_lines
            .saturating_sub(self.last_viewport_height);
    }

    pub fn jump_to_bottom(&mut self) {
        self.scroll_offset_lines = 0;
    }
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
    pub live_animation_frame: u64,
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
            live_animation_frame: 0,
            expand_tool_details: false,
            pre_llm_filter_enabled: false,
            permission_mode: "ReadOnly".to_string(),
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

#[cfg(test)]
mod tests {
    use super::TranscriptState;

    #[test]
    fn transcript_scroll_tracks_new_lines_while_scrolled_up() {
        let mut state = TranscriptState::default();
        state.set_viewport_height(10);
        state.note_total_lines(40);
        state.scroll_up(5);

        state.note_total_lines(45);

        assert_eq!(state.scroll_offset_lines, 10);
    }

    #[test]
    fn transcript_scroll_clamps_to_available_history() {
        let mut state = TranscriptState::default();
        state.set_viewport_height(10);
        state.note_total_lines(12);
        state.scroll_up(50);

        assert_eq!(state.scroll_offset_lines, 2);
    }
}
