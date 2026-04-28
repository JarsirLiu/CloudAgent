pub use crate::approval_overlay::ApprovalInlineState;
use crate::approval_overlay::ApprovalOverlay;
use crate::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::chat_composer::{ChatComposer, ComposerAction};
use crate::footer::{divider_line, hint_line, status_line};
use agent_protocol::FrontendMode;
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

#[derive(Clone, Debug, Default)]
pub struct InputPaneViewState {
    pub title: String,
    pub lines: Vec<String>,
}

pub struct InputPane {
    composer: ChatComposer,
    view_stack: Vec<Box<dyn BottomPaneView>>,
    approval_active: bool,
}

pub enum InputPaneAction {
    Composer(ComposerAction),
    ApprovalSubmit { approved: bool, reason: String },
}

impl InputPane {
    pub fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
            view_stack: Vec::new(),
            approval_active: false,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<InputPaneAction> {
        if let Some(view) = self.view_stack.last_mut() {
            match view.handle_key_event(key) {
                BottomPaneViewAction::None => {}
                BottomPaneViewAction::Close => {
                    self.approval_active = false;
                    self.view_stack.pop();
                    return Some(InputPaneAction::Composer(ComposerAction::None));
                }
                BottomPaneViewAction::ApprovalSubmit { approved, reason } => {
                    self.approval_active = false;
                    self.view_stack.pop();
                    return Some(InputPaneAction::ApprovalSubmit { approved, reason });
                }
            }
            if view.is_complete() {
                self.approval_active = false;
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
        lines.push(status_line(mode, status_text, status_meta));
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
            lines.push(hint_line(mode));
        }

        let total_lines = lines.len() as u16;
        (
            Paragraph::new(Text::from(lines))
                .block(Block::default().borders(Borders::NONE))
                .wrap(Wrap { trim: false }),
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

    pub fn set_panel(&mut self, panel: Option<InputPaneViewState>) {
        self.clear_non_approval_views();
        if let Some(panel) = panel {
            self.view_stack.push(Box::new(InfoOverlay::new(panel)));
        }
    }

    pub fn clear_views(&mut self) {
        self.view_stack.clear();
        self.approval_active = false;
    }

    pub fn set_approval(&mut self, approval: ApprovalInlineState) {
        self.clear_non_approval_views();
        self.view_stack
            .push(Box::new(ApprovalOverlay::new(approval)));
        self.approval_active = true;
    }

    pub fn clear_approval(&mut self) {
        if self.approval_active {
            self.view_stack.clear();
            self.approval_active = false;
        }
    }

    pub fn composer_is_empty(&self) -> bool {
        self.view_stack.is_empty() && self.composer.is_empty()
    }

    fn clear_non_approval_views(&mut self) {
        if !self.approval_active {
            self.view_stack.clear();
        }
    }
}

struct InfoOverlay {
    state: InputPaneViewState,
}

impl InfoOverlay {
    fn new(state: InputPaneViewState) -> Self {
        Self { state }
    }
}

impl BottomPaneView for InfoOverlay {
    fn render_lines(&self, _area_width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    self.state.title.clone(),
                    Style::default()
                        .fg(Color::Rgb(210, 210, 220))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::raw(""),
        ];
        lines.extend(self.state.lines.iter().map(|line| {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(line.clone(), Style::default().fg(Color::Rgb(130, 130, 145))),
            ])
        }));
        lines.push(Line::raw(""));
        lines
    }
}
