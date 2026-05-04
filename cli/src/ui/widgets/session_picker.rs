use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::textarea::display_width;
use agent_protocol::ConversationSummary;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct SessionPicker {
    sessions: Vec<ConversationSummary>,
    selected: usize,
    mode: SessionPickerMode,
}

const MAX_VISIBLE_SESSIONS: usize = 6;

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
        sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        let selected = sessions
            .iter()
            .position(|s| s.conversation_id == active_id)
            .unwrap_or(0);
        Self {
            sessions,
            selected,
            mode,
        }
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
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Close,
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
                Style::default()
                    .fg(Color::Rgb(190, 220, 255))
                    .bg(Color::Rgb(26, 34, 50))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(135, 145, 175))
            };
            lines.push(Line::from(vec![
                Span::raw(format!("  ")),
                Span::styled(row, style),
            ]));
        }
        if end < self.sessions.len() {
            lines.push(Line::from("  ..."));
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
        height
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
