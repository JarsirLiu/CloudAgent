use crate::input::intent::ComposerIntent;
use agent_protocol::ServerRequestDecisionKind;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::text::Line;

#[derive(Debug, Clone)]
pub(crate) enum BottomPaneViewAction {
    None,
    Close,
    Composer(ComposerIntent),
    ServerRequestSubmit {
        decision: ServerRequestDecisionKind,
        reason: String,
    },
}

pub(crate) trait BottomPaneView {
    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if matches!(key.kind, KeyEventKind::Press)
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        {
            return BottomPaneViewAction::Close;
        }
        BottomPaneViewAction::None
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>>;

    fn cursor_position(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }

    fn is_complete(&self) -> bool {
        false
    }
}
