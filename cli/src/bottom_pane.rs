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

    /// Renders the pane and returns (Widget, lines_before_input, total_lines)
    pub fn render(
        &self,
        mode: FrontendMode,
        status_text: &str,
        approval: Option<&ApprovalInlineState>,
        area_width: u16,
    ) -> (Paragraph<'static>, u16, u16) {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // 1. Divider
        lines.push(divider_line(area_width as usize));

        // 2. Status Line
        lines.push(self.status_line(mode, status_text));
        lines.push(Line::raw(""));

        let mut lines_before_composer = 3u16;

        // 3. Main Content
        if let Some(panel) = self.view_stack.last() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(panel.title.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::raw(""));
            lines_before_composer += 2;
            for l in &panel.lines {
                lines.push(Line::from(vec![Span::raw("  "), Span::styled(l.clone(), Style::default().fg(Color::Gray))]));
                lines_before_composer += 1;
            }
            lines.push(Line::raw(""));
            lines_before_composer += 1;
        } else if let Some(approval) = approval {
            let accent = Color::Rgb(255, 180, 50);
            lines.push(Line::from(vec![
                Span::styled("  ┌─ ACTION REQUIRED ──────────────────────────────────────────", Style::default().fg(accent)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(accent)),
                Span::styled(approval.title.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(accent)),
                Span::styled(approval.detail.clone(), Style::default().fg(Color::Rgb(160, 160, 170))),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(accent)),
                Span::styled("  [y] ", Style::default().fg(Color::Rgb(100, 255, 100)).add_modifier(Modifier::BOLD)),
                Span::styled("Approve  ", Style::default().fg(Color::White)),
                Span::styled("[n] ", Style::default().fg(Color::Rgb(255, 100, 100)).add_modifier(Modifier::BOLD)),
                Span::styled("Reject", Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  └────────────────────────────────────────────────────────────", Style::default().fg(accent)),
            ]));
            lines.push(Line::raw(""));
            lines_before_composer += 6;
        }

        // 4. Composer
        let composer_lines = self.composer.render_lines(mode, area_width.saturating_sub(4) as usize);
        lines.extend(composer_lines);

        let total_lines = lines.len() as u16;

        (
            Paragraph::new(Text::from(lines))
                .block(Block::default().borders(Borders::NONE))
                .wrap(Wrap { trim: false }),
            lines_before_composer,
            total_lines
        )
    }

    pub fn cursor_position(&self, area: Rect, lines_before: u16) -> (u16, u16) {
        let inner = Rect {
            x: area.x,
            y: area.y + lines_before,
            width: area.width,
            height: area.height.saturating_sub(lines_before),
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
    ) -> Line<'static> {
        let (dot_color, mode_label) = match mode {
            FrontendMode::Idle => (Color::Rgb(80, 200, 120), "IDLE"),
            FrontendMode::Running => (Color::Rgb(100, 160, 255), "RUNNING"),
            FrontendMode::WaitingForApproval => (Color::Rgb(255, 180, 50), "APPROVAL"),
        };

        let status_spans = if mode == FrontendMode::Running {
            shimmer_spans(status_text)
        } else {
            vec![Span::styled(status_text.to_string(), Style::default().fg(Color::Rgb(150, 150, 160)))]
        };

        let mut spans = vec![
            Span::raw("  "),
            Span::styled("● ", Style::default().fg(dot_color)),
            Span::styled(format!("{mode_label} "), Style::default().fg(dot_color).add_modifier(Modifier::BOLD)),
            Span::styled(" · ", Style::default().fg(Color::Rgb(60, 60, 70))),
        ];
        spans.extend(status_spans);

        Line::from(spans)
    }
}

fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled("─".repeat(width), Style::default().fg(Color::Rgb(40, 40, 50))))
}
