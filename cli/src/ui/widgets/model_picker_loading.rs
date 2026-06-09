use crate::ui::widgets::bottom_pane_view::BottomPaneView;
use ratatui::style::{Color, Style};
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
    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        vec![
            Line::from("  Model Picker"),
            Line::from(format!("  Current model: {}", self.current)),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Loading model list...",
                    Style::default().fg(Color::Rgb(135, 145, 175)),
                ),
            ]),
            Line::from("  Esc to cancel"),
        ]
    }

    fn is_model_picker_loading(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "model_picker_loading_tests.rs"]
mod tests;
