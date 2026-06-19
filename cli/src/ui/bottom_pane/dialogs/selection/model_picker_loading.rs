use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, ViewKind};
use crate::ui::theme::picker_unselected_style;
use ratatui::text::{Line, Span};

pub(crate) struct ModelPickerLoading {
    current: String,
}

impl ModelPickerLoading {
    pub(crate) fn new(current: impl Into<String>) -> Self {
        Self {
            current: current.into(),
        }
    }
}

impl BottomPaneView for ModelPickerLoading {
    fn kind(&self) -> ViewKind {
        ViewKind::ModelPickerLoading
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        vec![
            Line::from("  Model Picker"),
            Line::from(format!("  Current model: {}", self.current)),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Loading model list...", picker_unselected_style()),
            ]),
            Line::from("  Esc to cancel"),
        ]
    }
}

#[cfg(test)]
#[path = "model_picker_loading_tests.rs"]
mod tests;

