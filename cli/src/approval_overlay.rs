use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::bottom_pane_view::{BottomPaneView, BottomPaneViewAction};
use crate::textarea::TextArea;

#[derive(Clone, Debug, Default)]
pub struct ApprovalInlineState {
    pub title: String,
    pub detail: String,
}

pub struct ApprovalOverlay {
    state: ApprovalInlineState,
    reply: TextArea,
    complete: bool,
    selected: usize,
}

impl ApprovalOverlay {
    pub fn new(state: ApprovalInlineState) -> Self {
        Self {
            state,
            reply: TextArea::new(),
            complete: false,
            selected: 0,
        }
    }
}

impl BottomPaneView for ApprovalOverlay {
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
                self.selected = self.selected.saturating_add(1).min(1);
                BottomPaneViewAction::None
            }
            KeyCode::Char('y') if self.reply.is_empty() => {
                self.complete = true;
                BottomPaneViewAction::ApprovalSubmit {
                    approved: true,
                    reason: String::new(),
                }
            }
            KeyCode::Char('n') if self.reply.is_empty() => {
                self.complete = true;
                BottomPaneViewAction::ApprovalSubmit {
                    approved: false,
                    reason: String::new(),
                }
            }
            KeyCode::Enter => {
                let reason = self.reply.take_trimmed();
                let approved = if reason.is_empty() {
                    self.selected == 0
                } else {
                    !reason.eq_ignore_ascii_case("n") && !reason.eq_ignore_ascii_case("no")
                };
                self.complete = true;
                BottomPaneViewAction::ApprovalSubmit { approved, reason }
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
            "Type y / n, or enter a short reason".to_string()
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
                        .fg(Color::Rgb(255, 100, 100))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Deny", option_style(self.selected == 1)),
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
                "  Up/Down select  ·  Enter submit  ·  y approve  ·  n deny",
                Style::default().fg(Color::Rgb(62, 62, 78)),
            )),
        ]
    }

    fn cursor_position(&self, area: Rect) -> Option<(u16, u16)> {
        let prompt_width = 10usize;
        let x = area.x
            + prompt_width as u16
            + unicode_width::UnicodeWidthStr::width(self.reply.text()) as u16;
        let y = area.y + 7;
        Some((x, y))
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
}
