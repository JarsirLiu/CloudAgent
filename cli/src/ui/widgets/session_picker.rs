use std::cmp::Reverse;

use crate::input::intent::ComposerIntent;
use crate::text_width::display_width;
use crate::ui::theme::{picker_selected_style, picker_unselected_style};
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewKind};
use agent_core::ConversationSummary;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::text::{Line, Span};

pub struct SessionPicker {
    sessions: Vec<ConversationSummary>,
    selected: usize,
    mode: SessionPickerMode,
    has_more: bool,
    next_cursor: Option<String>,
    loading_more: bool,
}

const MAX_VISIBLE_SESSIONS: usize = 6;
const LOAD_MORE_THRESHOLD: usize = 2;

#[derive(Clone, Copy)]
pub enum SessionPickerMode {
    Switch,
    Delete,
}

impl SessionPicker {
    pub fn new(
        mut sessions: Vec<ConversationSummary>,
        active_id: &str,
        mode: SessionPickerMode,
    ) -> Self {
        sessions.sort_by_key(|session| Reverse(session.updated_at_ms));
        let selected = sessions
            .iter()
            .position(|s| s.conversation_id == active_id)
            .unwrap_or(0);
        Self {
            sessions,
            selected,
            mode,
            has_more: false,
            next_cursor: None,
            loading_more: false,
        }
    }

    pub fn new_page(
        sessions: Vec<ConversationSummary>,
        active_id: &str,
        mode: SessionPickerMode,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> Self {
        let mut picker = Self::new(sessions, active_id, mode);
        picker.has_more = has_more;
        picker.next_cursor = next_cursor;
        picker
    }
}

impl SessionPicker {
    fn select_current(&self) -> Option<String> {
        self.sessions
            .get(self.selected)
            .map(|s| s.conversation_id.clone())
    }
}

impl BottomPaneView for SessionPicker {
    fn kind(&self) -> ViewKind {
        ViewKind::SessionPicker
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                BottomPaneViewAction::None
            }
            KeyCode::Down => {
                if self.selected + 1 < self.sessions.len() {
                    self.selected += 1;
                }
                if let Some(cursor) = self.next_page_cursor_if_needed() {
                    self.loading_more = true;
                    return BottomPaneViewAction::LoadMoreSessions { cursor };
                }
                BottomPaneViewAction::None
            }
            KeyCode::Enter => self
                .select_current()
                .map(|id| {
                    BottomPaneViewAction::Composer(match self.mode {
                        SessionPickerMode::Switch => ComposerIntent::SessionSwitch(id),
                        SessionPickerMode::Delete => ComposerIntent::DeleteConversation(id),
                    })
                })
                .unwrap_or(BottomPaneViewAction::None),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Cancel,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(match self.mode {
            SessionPickerMode::Switch => "  Session Picker",
            SessionPickerMode::Delete => "  Delete Session  permanently remove data",
        })];
        if matches!(self.mode, SessionPickerMode::Delete) {
            lines.push(Line::from("  Enter to delete, Esc to cancel"));
        }
        let (start, end) = self.visible_window(MAX_VISIBLE_SESSIONS);
        if start > 0 {
            lines.push(Line::from("  ..."));
        }
        let id_col = 24usize;
        let max_width = area_width as usize;
        for (idx, s) in self.sessions[start..end].iter().enumerate() {
            let absolute_idx = start + idx;
            let selected = absolute_idx == self.selected;
            let marker = if selected { "> " } else { "  " };
            let id = truncate_to_width(&s.conversation_id, id_col);
            let title = truncate_to_width(
                s.title.as_deref().unwrap_or(""),
                max_width.saturating_sub(id_col + 8),
            );
            let row = format!("{marker}{id:<id_col$}  {title}", id_col = id_col);
            let style = if selected {
                picker_selected_style()
            } else {
                picker_unselected_style()
            };
            lines.push(Line::from(vec![
                Span::raw("  ".to_string()),
                Span::styled(row, style),
            ]));
        }
        if end < self.sessions.len() {
            lines.push(Line::from("  ..."));
        }
        if self.loading_more {
            lines.push(Line::from("  Loading more sessions..."));
        } else if self.has_more {
            lines.push(Line::from("  more sessions below"));
        }
        lines
    }

    fn desired_height(&self, _area_width: u16) -> u16 {
        let visible = self.sessions.len().min(MAX_VISIBLE_SESSIONS) as u16;
        let mut height = 1 + visible;
        if matches!(self.mode, SessionPickerMode::Delete) {
            height += 1;
        }
        if self.sessions.len() > MAX_VISIBLE_SESSIONS {
            height += 2;
        }
        if self.loading_more || self.has_more {
            height += 1;
        }
        height
    }

    fn append_session_page(
        &mut self,
        sessions: Vec<ConversationSummary>,
        has_more: bool,
        next_cursor: Option<String>,
    ) -> bool {
        for session in sessions {
            if self
                .sessions
                .iter()
                .any(|existing| existing.conversation_id == session.conversation_id)
            {
                continue;
            }
            self.sessions.push(session);
        }
        self.sessions
            .sort_by_key(|session| Reverse(session.updated_at_ms));
        self.has_more = has_more;
        self.next_cursor = next_cursor;
        self.loading_more = false;
        true
    }
}

impl SessionPicker {
    fn visible_window(&self, max_rows: usize) -> (usize, usize) {
        if self.sessions.is_empty() || max_rows == 0 {
            return (0, 0);
        }
        let visible = self.sessions.len().min(max_rows);
        let start = if self.selected < visible {
            0
        } else {
            (self.selected + 1).saturating_sub(visible)
        }
        .min(self.sessions.len().saturating_sub(visible));
        (start, start + visible)
    }

    fn next_page_cursor_if_needed(&self) -> Option<String> {
        if self.loading_more || !self.has_more {
            return None;
        }
        let cursor = self.next_cursor.clone()?;
        let remaining = self.sessions.len().saturating_sub(self.selected + 1);
        if remaining <= LOAD_MORE_THRESHOLD {
            Some(cursor)
        } else {
            None
        }
    }
}

fn truncate_to_width(value: &str, width: usize) -> String {
    if width == 0 || display_width(value) <= width {
        return value.to_string();
    }
    let mut out = String::new();
    for ch in value.chars() {
        let next = format!("{out}{ch}");
        if display_width(&next) + 3 > width {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
#[path = "session_picker_tests.rs"]
mod tests;
