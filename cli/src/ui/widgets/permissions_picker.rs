use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct PermissionsPicker {
    selected: usize,
    options: [&'static str; 3],
}

impl PermissionsPicker {
    pub fn new(current: &str) -> Self {
        let options = ["safe", "balanced", "danger"];
        let selected = options.iter().position(|m| *m == current).unwrap_or(0);
        Self { selected, options }
    }
}

impl BottomPaneView for PermissionsPicker {
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
            KeyCode::Enter => BottomPaneViewAction::Composer(ComposerIntent::Permissions(
                self.options[self.selected].to_string(),
            )),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Close,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from("  Permissions Picker (session-scoped)")];
        for (idx, mode) in self.options.iter().enumerate() {
            let selected = idx == self.selected;
            let marker = if selected { "> " } else { "  " };
            let style = if selected {
                Style::default()
                    .fg(Color::Rgb(190, 220, 255))
                    .bg(Color::Rgb(26, 34, 50))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(135, 145, 175))
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{marker}{mode}"), style),
            ]));
        }
        lines
    }
}
