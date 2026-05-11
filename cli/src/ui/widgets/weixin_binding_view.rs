use crate::input::intent::ComposerIntent;
use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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
    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                BottomPaneViewAction::Composer(ComposerIntent::GatewaySelect(
                    self.model.platform.clone(),
                ))
            }
            _ => BottomPaneViewAction::None,
        }
    }

    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let title = Style::default()
            .fg(Color::Rgb(190, 220, 255))
            .add_modifier(Modifier::BOLD);
        let body = Style::default().fg(Color::Rgb(210, 215, 225));
        let dim = Style::default().fg(Color::Rgb(140, 150, 180));
        vec![
            Line::from(Span::styled("  Weixin Binding", title)),
            Line::from("  Scan with WeChat on your phone. This page checks status automatically."),
            Line::from(format!("  Session: {}", self.model.session_id)),
            Line::from("  Scan URL:"),
            Line::from(Span::styled(format!("  {}", self.model.qr_url), body)),
            Line::from("  "),
            Line::from(vec![
                Span::styled("  Status: ", dim),
                Span::styled(self.model.status.clone(), body),
            ]),
            Line::from(Span::styled(
                "  Esc returns to the gateway page.",
                dim,
            )),
        ]
    }

    fn cursor_position(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}
