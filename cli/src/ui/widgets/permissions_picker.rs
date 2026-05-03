use crate::app::commands::permission_profile::{
    DEFAULT_PERMISSION_MODE, PERMISSION_MODE_SPECS, PermissionModeSpec,
};
use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub struct PermissionsPicker {
    selected: usize,
    options: &'static [PermissionModeSpec],
}

impl PermissionsPicker {
    pub fn new(current: &str) -> Self {
        let options = &PERMISSION_MODE_SPECS;
        let selected = options
            .iter()
            .position(|m| m.mode == current)
            .or_else(|| options.iter().position(|m| m.mode == DEFAULT_PERMISSION_MODE))
            .unwrap_or(0);
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
                self.options[self.selected].mode.to_string(),
            )),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Close,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from("  Permissions Picker (session-scoped)"),
            Line::from("  Choose how broad tool execution can be in this session"),
        ];
        for (idx, spec) in self.options.iter().enumerate() {
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
                Span::styled(format!("{marker}{:<9}", spec.mode), style),
                Span::styled(
                    spec.label.to_string(),
                    Style::default().fg(Color::Rgb(120, 130, 150)),
                ),
            ]));
        }
        lines
    }
}
