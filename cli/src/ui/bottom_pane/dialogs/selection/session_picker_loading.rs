use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, ViewKind};
use crate::ui::bottom_pane::dialogs::selection::session_picker::SessionPickerMode;
use crate::ui::theme::picker_unselected_style;
use ratatui::text::{Line, Span};

pub(crate) struct SessionPickerLoading {
    mode: SessionPickerMode,
}

impl SessionPickerLoading {
    pub(crate) fn new(mode: SessionPickerMode) -> Self {
        Self { mode }
    }
}

impl BottomPaneView for SessionPickerLoading {
    fn kind(&self) -> ViewKind {
        ViewKind::SessionPickerLoading
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let title = match self.mode {
            SessionPickerMode::Switch => "  Session Picker",
            SessionPickerMode::Delete => "  Delete Session",
        };
        vec![
            Line::from(title),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("Loading sessions...", picker_unselected_style()),
            ]),
            Line::from("  Esc to cancel"),
        ]
    }
}

#[cfg(test)]
#[path = "session_picker_loading_tests.rs"]
mod tests;
