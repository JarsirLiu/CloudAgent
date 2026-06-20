use crate::input::intent::ComposerIntent;
use crate::ui::bottom_pane::dialogs::server_request::server_request_model::ServerRequestInlineState;
use agent_core::ServerRequestDecisionKind;
use agent_protocol::RequestId;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::text::Line;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViewCompletion {
    Accepted,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViewKind {
    Help,
    Filter,
    Config,
    Permissions,
    Reasoning,
    ModelPicker,
    ModelPickerLoading,
    SessionPicker,
    SessionPickerLoading,
    GatewayList,
    GatewayEdit,
    WeixinBinding,
    ServerRequest,
}

#[derive(Debug, Clone)]
pub(crate) enum BottomPaneViewAction {
    None,
    Handled,
    Cancel,
    Back,
    Composer(ComposerIntent),
    ComposerWithoutDismiss(ComposerIntent),
    LoadMoreSessions {
        cursor: String,
    },
    ServerRequestSubmit {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
}

pub(crate) trait BottomPaneView {
    fn kind(&self) -> ViewKind;

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if matches!(key.kind, KeyEventKind::Press)
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        {
            return BottomPaneViewAction::Cancel;
        }
        BottomPaneViewAction::None
    }

    fn handle_paste(&mut self, _text: &str) -> BottomPaneViewAction {
        BottomPaneViewAction::None
    }

    fn should_capture_global_paste_shortcut(&self) -> bool {
        false
    }

    fn supports_text_paste_shortcut(&self) -> bool {
        false
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>>;

    fn desired_height(&self, area_width: u16) -> u16 {
        self.render_lines(area_width).len() as u16
    }

    fn cursor_position(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }

    fn is_complete(&self) -> bool {
        false
    }

    fn completion(&self) -> Option<ViewCompletion> {
        None
    }

    fn dismiss_after_child_accept(&self) -> bool {
        false
    }

    fn clear_dismiss_after_child_accept(&mut self) {}

    fn try_consume_server_request(
        &mut self,
        request: ServerRequestInlineState,
    ) -> Option<ServerRequestInlineState> {
        Some(request)
    }

    fn dismiss_server_request(&mut self, _request_id: &RequestId) -> bool {
        false
    }

    fn active_server_request_id(&self) -> Option<&RequestId> {
        None
    }

    fn append_session_page(
        &mut self,
        _sessions: Vec<agent_core::ConversationSummary>,
        _has_more: bool,
        _next_cursor: Option<String>,
    ) -> bool {
        false
    }
}
