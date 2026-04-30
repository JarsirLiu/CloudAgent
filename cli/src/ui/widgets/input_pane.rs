use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::chat_composer::{ChatComposer, ComposerAction};
use crate::ui::widgets::footer::{divider_line, hint_line, status_line};
pub use crate::ui::widgets::server_request_overlay::ServerRequestInlineState;
use crate::ui::widgets::server_request_overlay::ServerRequestOverlay;
use agent_protocol::{FrontendMode, ServerRequestDecisionKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

pub struct InputPane {
    composer: ChatComposer,
    view_stack: Vec<Box<dyn BottomPaneView>>,
}

pub enum InputPaneAction {
    Composer(ComposerAction),
    ServerRequestSubmit {
        decision: ServerRequestDecisionKind,
        reason: String,
    },
}

impl InputPane {
    pub fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
            view_stack: Vec::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
            return Some(InputPaneAction::Composer(ComposerAction::Interrupt));
        }

        if let Some(view) = self.view_stack.last_mut() {
            match view.handle_key_event(key) {
                BottomPaneViewAction::None => {}
                BottomPaneViewAction::Close => {
                    self.view_stack.pop();
                    return Some(InputPaneAction::Composer(ComposerAction::None));
                }
                BottomPaneViewAction::ServerRequestSubmit { decision, reason } => {
                    self.view_stack.pop();
                    return Some(InputPaneAction::ServerRequestSubmit { decision, reason });
                }
            }
            if view.is_complete() {
                self.view_stack.pop();
            }
            return None;
        }
        self.composer.handle_key(key).map(InputPaneAction::Composer)
    }

    pub fn render(
        &self,
        mode: FrontendMode,
        status_text: &str,
        status_meta: &str,
        area_width: u16,
    ) -> (Paragraph<'static>, u16, u16) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(divider_line(area_width as usize));
        lines.push(status_line(
            mode,
            status_text,
            status_meta,
            area_width as usize,
        ));
        lines.push(Line::raw(""));

        let mut lines_before_composer = 3u16;

        if let Some(view) = self.view_stack.last() {
            let view_lines = view.render_lines(area_width);
            lines_before_composer += view_lines.len() as u16;
            lines.extend(view_lines);
        }

        if self.view_stack.is_empty() {
            let composer = self
                .composer
                .render(mode, area_width.saturating_sub(4) as usize);
            lines_before_composer += composer.cursor_row;
            lines.extend(composer.lines);
            lines.push(hint_line(mode, area_width as usize));
        }

        let total_lines = lines.len() as u16;
        (
            Paragraph::new(Text::from(lines)).block(Block::default().borders(Borders::NONE)),
            lines_before_composer,
            total_lines,
        )
    }

    pub fn desired_height(&self, mode: FrontendMode, area_width: u16) -> u16 {
        let mut total = 3u16;
        if let Some(view) = self.view_stack.last() {
            total += view.render_lines(area_width).len() as u16;
        } else {
            total += self
                .composer
                .desired_height(mode, area_width.saturating_sub(4) as usize);
            total += 1; // hint line
        }
        total.max(6)
    }

    pub fn cursor_position(&self, area: Rect, lines_before: u16, mode: FrontendMode) -> (u16, u16) {
        let inner = Rect {
            x: area.x,
            y: area.y + lines_before,
            width: area.width,
            height: area.height.saturating_sub(lines_before),
        };
        if let Some(view) = self.view_stack.last() {
            return view.cursor_position(area).unwrap_or((inner.x, inner.y));
        }
        self.composer.cursor_position(inner, mode)
    }

    pub fn clear_views(&mut self) {
        self.view_stack.clear();
    }

    pub fn set_server_request(&mut self, request: ServerRequestInlineState) {
        self.view_stack.clear();
        self.view_stack
            .push(Box::new(ServerRequestOverlay::new(request)));
    }

    pub fn clear_server_request(&mut self) {
        self.view_stack.clear();
    }

    pub fn composer_is_empty(&self) -> bool {
        self.view_stack.is_empty() && self.composer.is_empty()
    }
}
