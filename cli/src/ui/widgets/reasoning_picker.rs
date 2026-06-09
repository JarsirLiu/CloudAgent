use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use config::ReasoningEffort;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct ReasoningPicker {
    selected: usize,
    options: [ReasoningEffort; 3],
}

impl ReasoningPicker {
    pub fn new(current: ReasoningEffort) -> Self {
        let options = [
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
        ];
        let selected = options
            .iter()
            .position(|option| *option == current)
            .unwrap_or(1);
        Self { selected, options }
    }
}

impl BottomPaneView for ReasoningPicker {
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
            KeyCode::Enter => BottomPaneViewAction::Composer(ComposerIntent::Reasoning(
                self.options[self.selected].to_string(),
            )),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Cancel,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(format!(
                "  Reasoning Picker  selected: {}",
                self.options[self.selected]
            )),
            Line::from("  Choose how much reasoning the model should spend"),
        ];
        for (idx, option) in self.options.iter().enumerate() {
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
                Span::styled(format!("{marker}{option}"), style),
            ]));
        }
        lines.push(Line::from("  Enter to apply, Esc to cancel"));
        lines
    }
}
