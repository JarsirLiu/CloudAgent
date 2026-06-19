use crate::input::intent::ComposerIntent;
use crate::ui::theme::{picker_selected_style, picker_unselected_style};
use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewKind};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::text::{Line, Span};

pub struct FilterPicker {
    selected: usize,
    options: [&'static str; 2],
}

impl FilterPicker {
    pub fn new() -> Self {
        Self {
            selected: 0,
            options: ["on", "off"],
        }
    }
}

impl Default for FilterPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl BottomPaneView for FilterPicker {
    fn kind(&self) -> ViewKind {
        ViewKind::Filter
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
                if self.selected + 1 < self.options.len() {
                    self.selected += 1;
                }
                BottomPaneViewAction::None
            }
            KeyCode::Enter => BottomPaneViewAction::Composer(ComposerIntent::Filter(
                self.options[self.selected].to_string(),
            )),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Cancel,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from("  Filter Picker")];
        for (idx, option) in self.options.iter().enumerate() {
            let selected = idx == self.selected;
            let marker = if selected { "> " } else { "  " };
            let style = if selected {
                picker_selected_style()
            } else {
                picker_unselected_style()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{marker}{option}"), style),
            ]));
        }
        lines
    }
}

