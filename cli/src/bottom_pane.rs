use crate::composer::{Composer, ComposerAction};
use crate::history_cell::shimmer_spans;
use agent_protocol::FrontendMode;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

#[derive(Clone, Debug, Default)]
pub struct ApprovalInlineState {
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default)]
pub struct BottomPaneViewState {
    pub title: String,
    pub lines: Vec<String>,
}

pub struct BottomPane {
    composer: Composer,
    view_stack: Vec<BottomPaneViewState>,
}

impl BottomPane {
    pub fn new() -> Self {
        Self {
            composer: Composer::new(),
            view_stack: Vec::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<ComposerAction> {
        if !self.view_stack.is_empty()
            && matches!(key.kind, KeyEventKind::Press)
            && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        {
            self.view_stack.pop();
            return Some(ComposerAction::None);
        }
        self.composer.handle_key(key)
    }

    pub fn render(
        &self,
        mode: FrontendMode,
        status_text: &str,
        approval: Option<&ApprovalInlineState>,
        area_width: u16,
    ) -> Paragraph<'static> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // ── Divider ──────────────────────────────────────────────────────────
        lines.push(divider_line(area_width as usize));

        // ── Status row ───────────────────────────────────────────────────────
        lines.push(self.status_line(mode, status_text, approval.is_some()));
        lines.push(Line::raw(""));

        // ── Body ─────────────────────────────────────────────────────────────
        if let Some(panel) = self.view_stack.last() {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    panel.title.clone(),
                    Style::default()
                        .fg(Color::Rgb(200, 200, 210))
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::raw(""));
            for l in &panel.lines {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(l.clone(), Style::default().fg(Color::Rgb(140, 140, 150))),
                ]));
            }
        } else if let Some(approval) = approval {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Approval required",
                    Style::default()
                        .fg(Color::Rgb(255, 180, 50))
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(approval.title.clone(), Style::default().fg(Color::Rgb(220, 220, 220))),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(approval.detail.clone(), Style::default().fg(Color::Rgb(140, 140, 150))),
            ]));
            lines.push(Line::raw(""));
            lines.extend(
                self.composer
                    .render_lines(mode, area_width.saturating_sub(4) as usize),
            );
        } else {
            lines.extend(
                self.composer
                    .render_lines(mode, area_width.saturating_sub(4) as usize),
            );
        }

        Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::NONE))
            .wrap(Wrap { trim: false })
    }

    pub fn cursor_position(&self, area: Rect) -> (u16, u16) {
        if !self.view_stack.is_empty() {
            return (area.x + 2, area.y + 2);
        }
        // divider(1) + status(1) + blank(1) = 3 rows before composer
        let inner = Rect {
            x: area.x + 2,
            y: area.y + 3,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(5),
        };
        self.composer.cursor_position(inner)
    }

    pub fn set_panel(&mut self, panel: Option<BottomPaneViewState>) {
        self.view_stack.clear();
        if let Some(panel) = panel {
            self.view_stack.push(panel);
        }
    }

    pub fn clear_views(&mut self) {
        self.view_stack.clear();
    }

    fn status_line(
        &self,
        mode: FrontendMode,
        status_text: &str,
        has_inline_approval: bool,
    ) -> Line<'static> {
        let (dot_color, mode_label) = match mode {
            FrontendMode::Idle => (Color::Rgb(80, 200, 120), "IDLE"),
            FrontendMode::Running => (Color::Rgb(100, 160, 255), "RUNNING"),
            FrontendMode::WaitingForApproval => (Color::Rgb(255, 180, 50), "APPROVAL"),
        };

        let hint = if !self.view_stack.is_empty() {
            "q / Esc  close".to_string()
        } else {
            match mode {
                FrontendMode::Idle => "Enter  send  ·  Ctrl+K  interrupt  ·  F2  history".to_string(),
                FrontendMode::Running => "Ctrl+K  interrupt".to_string(),
                FrontendMode::WaitingForApproval if has_inline_approval => {
                    "y  approve  ·  n  deny".to_string()
                }
                FrontendMode::WaitingForApproval => "Waiting for approval".to_string(),
            }
        };

        let status_spans: Vec<Span<'static>> = if mode == FrontendMode::Running {
            shimmer_spans(status_text)
        } else {
            vec![Span::styled(
                status_text.to_string(),
                Style::default().fg(Color::Rgb(150, 150, 160)),
            )]
        };

        let mut spans = vec![
            Span::raw("  "),
            Span::styled("● ", Style::default().fg(dot_color)),
            Span::styled(
                format!("{mode_label} "),
                Style::default()
                    .fg(dot_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Connected via in-process  ·  ", Style::default().fg(Color::Rgb(70, 70, 80))),
        ];
        spans.extend(status_spans);
        spans.push(Span::raw("  "));

        // Hint at right side (just appended — terminal will clip if too wide)
        spans.push(Span::styled(hint, Style::default().fg(Color::Rgb(70, 70, 80))));

        Line::from(spans)
    }
}

/// A subtle full-width divider line.
fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(Color::Rgb(45, 45, 55)),
    ))
}
