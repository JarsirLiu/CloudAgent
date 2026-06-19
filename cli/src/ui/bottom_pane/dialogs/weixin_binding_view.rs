use crate::ui::bottom_pane::bottom_pane_view::{BottomPaneView, BottomPaneViewAction, ViewKind};
use crate::ui::theme::{body_style, hint_style, title_style};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinBindingViewModel {
    pub platform: String,
    pub session_id: String,
    pub qr_url: String,
    pub status: String,
}

pub struct WeixinBindingView {
    model: WeixinBindingViewModel,
}

impl WeixinBindingView {
    pub fn new(model: WeixinBindingViewModel) -> Self {
        Self { model }
    }

    pub fn update(&mut self, model: WeixinBindingViewModel) {
        self.model = model;
    }
}

impl BottomPaneView for WeixinBindingView {
    fn kind(&self) -> ViewKind {
        ViewKind::WeixinBinding
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => BottomPaneViewAction::Back,
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled("  Weixin Binding", title_style())),
            Line::from("  Scan with WeChat on your phone. This page checks status automatically."),
            Line::from(format!("  Session: {}", self.model.session_id)),
            Line::from("  Scan URL:"),
            Line::from(Span::styled(
                format!("  {}", self.model.qr_url),
                body_style(),
            )),
            Line::from("  "),
            Line::from(vec![
                Span::styled("  Status: ", hint_style()),
                Span::styled(self.model.status.clone(), body_style()),
            ]),
            Line::from(Span::styled(
                "  Esc returns to the gateway page.",
                hint_style(),
            )),
        ]
    }

    fn cursor_position(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}
