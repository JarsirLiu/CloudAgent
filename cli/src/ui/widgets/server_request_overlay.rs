use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::widgets::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::ui::widgets::textarea::TextArea;
use agent_protocol::ServerRequestDecisionKind;

#[derive(Clone, Debug, Default)]
pub struct ServerRequestInlineState {
    pub title: String,
    pub detail: String,
}

pub struct ServerRequestOverlay {
    state: ServerRequestInlineState,
    reply: TextArea,
    complete: bool,
    selected: usize,
}

impl ServerRequestOverlay {
    pub fn new(state: ServerRequestInlineState) -> Self {
        Self {
            state,
            reply: TextArea::new(),
            complete: false,
            selected: 0,
        }
    }
}

impl BottomPaneView for ServerRequestOverlay {
    fn handle_key_event(&mut self, key: KeyEvent) -> BottomPaneViewAction {
        if !matches!(key.kind, KeyEventKind::Press) {
            return BottomPaneViewAction::None;
        }

        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                BottomPaneViewAction::None
            }
            KeyCode::Down => {
                self.selected = self.selected.saturating_add(1).min(2);
                BottomPaneViewAction::None
            }
            KeyCode::Char('y') if self.reply.is_empty() => {
                self.complete = true;
                BottomPaneViewAction::ServerRequestSubmit {
                    decision: ServerRequestDecisionKind::Accept,
                    reason: String::new(),
                }
            }
            KeyCode::Char('a') if self.reply.is_empty() => {
                self.complete = true;
                BottomPaneViewAction::ServerRequestSubmit {
                    decision: ServerRequestDecisionKind::AcceptForSession,
                    reason: String::new(),
                }
            }
            KeyCode::Char('n') if self.reply.is_empty() => {
                self.complete = true;
                BottomPaneViewAction::ServerRequestSubmit {
                    decision: ServerRequestDecisionKind::Decline,
                    reason: String::new(),
                }
            }
            KeyCode::Enter => {
                let reason = self.reply.take_trimmed();
                let decision = if reason.is_empty() {
                    selected_decision(self.selected)
                } else {
                    typed_decision(&reason)
                };
                self.complete = true;
                BottomPaneViewAction::ServerRequestSubmit { decision, reason }
            }
            _ => {
                self.reply.handle_key(key);
                BottomPaneViewAction::None
            }
        }
    }

    fn render_lines(&self, area_width: u16) -> Vec<Line<'static>> {
        let accent = Color::Rgb(255, 184, 76);
        let soft = Color::Rgb(150, 150, 160);
        let title_bg = Color::Rgb(45, 36, 18);
        let separator = "─".repeat(area_width.saturating_sub(4) as usize);
        let reply = if self.reply.is_empty() {
            "Type y / a / n, or enter a short reason".to_string()
        } else {
            self.reply.text().to_string()
        };
        let option_style = |selected: bool| {
            if selected {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(58, 46, 24))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(210, 210, 220))
            }
        };

        vec![
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    " ACTION REQUIRED ",
                    Style::default()
                        .fg(accent)
                        .bg(title_bg)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    self.state.title.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(self.state.detail.clone(), Style::default().fg(soft)),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 0 {
                        "  > 1."
                    } else {
                        "    1."
                    },
                    Style::default()
                        .fg(Color::Rgb(100, 255, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Approve once", option_style(self.selected == 0)),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 1 {
                        "  > 2."
                    } else {
                        "    2."
                    },
                    Style::default()
                        .fg(Color::Rgb(100, 210, 255))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Approve for session", option_style(self.selected == 1)),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    if self.selected == 2 {
                        "  > 3."
                    } else {
                        "    3."
                    },
                    Style::default()
                        .fg(Color::Rgb(255, 100, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Deny", option_style(self.selected == 2)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Reply",
                    Style::default()
                        .fg(Color::Rgb(180, 180, 195))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(separator, Style::default().fg(Color::Rgb(55, 55, 68))),
            ]),
            Line::from(vec![
                Span::styled(
                    "  reply   ",
                    Style::default()
                        .fg(accent)
                        .bg(title_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    reply,
                    if self.reply.is_empty() {
                        Style::default().fg(Color::Rgb(65, 65, 80))
                    } else {
                        Style::default().fg(Color::Rgb(220, 220, 230))
                    },
                ),
            ]),
            Line::from(Span::styled(
                "  Up/Down select  ·  Enter submit  ·  y approve  ·  a approve session  ·  n deny",
                Style::default().fg(Color::Rgb(62, 62, 78)),
            )),
        ]
    }

    fn cursor_position(&self, area: Rect) -> Option<(u16, u16)> {
        let prompt_width = 10usize;
        let mut x = area.x
            + prompt_width as u16
            + unicode_width::UnicodeWidthStr::width(self.reply.text()) as u16;
        let mut y = area.y + 8;
        if area.height > 0 {
            let max_y = area.y + area.height.saturating_sub(1);
            if y > max_y {
                y = max_y;
            }
        }
        if area.width > 0 {
            let max_x = area.x + area.width.saturating_sub(1);
            if x > max_x {
                x = max_x;
            }
        }
        Some((x, y))
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
}

fn selected_decision(selected: usize) -> ServerRequestDecisionKind {
    match selected {
        0 => ServerRequestDecisionKind::Accept,
        1 => ServerRequestDecisionKind::AcceptForSession,
        _ => ServerRequestDecisionKind::Decline,
    }
}

fn typed_decision(reason: &str) -> ServerRequestDecisionKind {
    if reason.eq_ignore_ascii_case("n") || reason.eq_ignore_ascii_case("no") {
        ServerRequestDecisionKind::Decline
    } else if reason.eq_ignore_ascii_case("a")
        || reason.eq_ignore_ascii_case("all")
        || reason.eq_ignore_ascii_case("session")
    {
        ServerRequestDecisionKind::AcceptForSession
    } else {
        ServerRequestDecisionKind::Accept
    }
}
