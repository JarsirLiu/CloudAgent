use crate::input::intent::ComposerIntent;
use crate::text_width::display_width;
use crate::ui::theme::{picker_current_style, picker_selected_style, picker_unselected_style};
use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewKind};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::text::{Line, Span};

pub struct ModelPicker {
    selected: usize,
    current: String,
    models: Vec<String>,
}

const MAX_VISIBLE_MODELS: usize = 8;

impl ModelPicker {
    pub fn new(current: impl Into<String>, models: Vec<String>) -> Self {
        let current = current.into();
        let selected = models
            .iter()
            .position(|model| model == &current)
            .unwrap_or(0);
        Self {
            selected,
            current,
            models,
        }
    }

    fn visible_window(&self) -> (usize, usize) {
        if self.models.is_empty() {
            return (0, 0);
        }
        let visible = self.models.len().min(MAX_VISIBLE_MODELS);
        let start = if self.selected < visible {
            0
        } else {
            (self.selected + 1).saturating_sub(visible)
        }
        .min(self.models.len().saturating_sub(visible));
        (start, start + visible)
    }
}

impl BottomPaneView for ModelPicker {
    fn kind(&self) -> ViewKind {
        ViewKind::ModelPicker
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
                if self.selected + 1 < self.models.len() {
                    self.selected += 1;
                }
                BottomPaneViewAction::None
            }
            KeyCode::Enter => BottomPaneViewAction::Composer(ComposerIntent::Model(
                self.models.get(self.selected).cloned().unwrap_or_default(),
            )),
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Cancel,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from("  Model Picker"),
            Line::from(format!("  Current model: {}", self.current)),
        ];
        let (start, end) = self.visible_window();
        if start > 0 {
            lines.push(Line::from("  ..."));
        }
        for (idx, model) in self.models[start..end].iter().enumerate() {
            let absolute_idx = start + idx;
            let selected = absolute_idx == self.selected;
            let is_current = model == &self.current;
            let marker = if selected { "> " } else { "  " };
            let mut label = model.clone();
            if is_current {
                label.push_str("  (current)");
            }
            let style = if selected {
                picker_selected_style()
            } else if is_current {
                picker_current_style()
            } else {
                picker_unselected_style()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    truncate_to_width(
                        &format!("{marker}{label}"),
                        area_width.saturating_sub(4) as usize,
                    ),
                    style,
                ),
            ]));
        }
        if end < self.models.len() {
            lines.push(Line::from("  ..."));
        }
        lines.push(Line::from("  Enter to apply, Esc to cancel"));
        lines
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

