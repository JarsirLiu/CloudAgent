use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::text::Line;

pub struct FilterPicker {
    selected: usize,
    options: [&'static str; 4],
}

impl FilterPicker {
    pub fn new() -> Self {
        Self {
            selected: 0,
            options: ["on", "off", "toggle", "status"],
        }
    }
}

impl BottomPaneView for FilterPicker {
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
                if self.selected + 1 < self.options.len() {
                    self.selected += 1;
                }
                BottomPaneViewAction::None
            }
            KeyCode::Enter => BottomPaneViewAction::Composer(ComposerIntent::Filter(
                self.options[self.selected].to_string(),
            )),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Close,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from("  Filter Picker (Enter apply, Esc close)")];
        for (idx, option) in self.options.iter().enumerate() {
            let marker = if idx == self.selected { ">" } else { " " };
            lines.push(Line::from(format!("  {marker} {option}")));
        }
        lines
    }
}
