use crate::ui::widgets::bottom_pane_view::BottomPaneView;
use crate::ui::widgets::session_picker::SessionPickerMode;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub(crate) struct SessionPickerLoading {
    generation: u64,
    mode: SessionPickerMode,
}

impl SessionPickerLoading {
    pub(crate) fn new(generation: u64, mode: SessionPickerMode) -> Self {
        Self { generation, mode }
    }
}

impl BottomPaneView for SessionPickerLoading {
    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let title = match self.mode {
            SessionPickerMode::Switch => "  Session Picker",
            SessionPickerMode::Delete => "  Delete Session",
        };
        vec![
            Line::from(title),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Loading sessions...",
                    Style::default().fg(Color::Rgb(135, 145, 175)),
                ),
            ]),
            Line::from("  Esc to cancel"),
        ]
    }

    fn is_session_picker(&self) -> bool {
        true
    }

    fn is_session_picker_loading(&self, generation: u64) -> bool {
        self.generation == generation
    }
}

#[cfg(test)]
#[path = "session_picker_loading_tests.rs"]
mod tests;
