use agent_protocol::ConversationSummary;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::text::{Line, Span};

pub struct SessionPicker {
    sessions: Vec<ConversationSummary>,
    selected: usize,
}

impl SessionPicker {
    pub fn new(mut sessions: Vec<ConversationSummary>, active_id: &str) -> Self {
        sessions.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        let selected = sessions
            .iter()
            .position(|s| s.conversation_id == active_id)
            .unwrap_or(0);
        Self { sessions, selected }
    }
}

impl SessionPicker {
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Option<String> {
        if !matches!(key.kind, KeyEventKind::Press) {
            return None;
        }
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                None
            }
            KeyCode::Down => {
                if self.selected + 1 < self.sessions.len() {
                    self.selected += 1;
                }
                None
            }
            KeyCode::Enter => self
                .sessions
                .get(self.selected)
                .map(|s| s.conversation_id.clone()),
            KeyCode::Esc | KeyCode::Char('q') => None,
            _ => None,
        }
    }

    pub fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from("  Session Picker (Enter switch, Esc close)")];
        for (idx, s) in self.sessions.iter().enumerate() {
            let marker = if idx == self.selected { ">" } else { " " };
            let title = s.title.clone().unwrap_or_default();
            lines.push(Line::from(vec![Span::raw(format!(
                "  {marker} {} {}",
                s.conversation_id,
                if title.is_empty() { "".to_string() } else { format!("[{title}]") }
            ))]));
        }
        lines
    }

    pub fn close_requested(key: &KeyEvent) -> bool {
        matches!(key.kind, KeyEventKind::Press) && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
    }
}
