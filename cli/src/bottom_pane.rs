use crate::composer::{Composer, ComposerAction};
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
        let mut lines = Vec::new();
        lines.push(self.status_line(mode, status_text, approval.is_some()));
        lines.push(Line::raw(""));

        if let Some(panel) = self.view_stack.last() {
            lines.push(Line::from(Span::styled(
                panel.title.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::raw(""));
            for line in &panel.lines {
                lines.push(Line::from(Span::styled(
                    line.clone(),
                    Style::default().fg(Color::Gray),
                )));
            }
        } else if let Some(approval) = approval {
            lines.push(Line::from(Span::styled(
                "Would you like to approve this action?",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled(
                    "approval  ",
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(approval.title.clone(), Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(Span::styled(
                approval.detail.clone(),
                Style::default().fg(Color::Gray),
            )));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "1. Approve once",
                Style::default().fg(Color::White),
            )));
            lines.push(Line::from(Span::styled(
                "2. Deny",
                Style::default().fg(Color::White),
            )));
            lines.push(Line::raw(""));
            lines.extend(
                self.composer
                    .render_lines(mode, area_width.saturating_sub(4) as usize),
            );
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "Approve with y or deny with n, then press Enter.",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.extend(
                self.composer
                    .render_lines(mode, area_width.saturating_sub(4) as usize),
            );
        }

        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(if approval.is_some() {
                        " Approval "
                    } else {
                        " Composer "
                    })
                    .title_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false })
    }

    pub fn cursor_position(&self, area: Rect) -> (u16, u16) {
        if !self.view_stack.is_empty() {
            return (area.x + 2, area.y + 2);
        }
        let inner = Rect {
            x: area.x + 1,
            y: area.y + if area.height > 6 { 6 } else { 3 },
            width: area.width.saturating_sub(2),
            height: area
                .height
                .saturating_sub(if area.height > 6 { 8 } else { 5 }),
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
        let mode_style = match mode {
            FrontendMode::Idle => Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
            FrontendMode::Running => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            FrontendMode::WaitingForApproval => Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        };
        let mode_label = match mode {
            FrontendMode::Idle => "IDLE",
            FrontendMode::Running => "RUNNING",
            FrontendMode::WaitingForApproval => "APPROVAL",
        };
        let hint = if !self.view_stack.is_empty() {
            "Esc close panel"
        } else {
            match mode {
                FrontendMode::Idle => "Ready",
                FrontendMode::Running => "Ctrl+K interrupt",
                FrontendMode::WaitingForApproval if has_inline_approval => {
                    "Choose 1 / 2 or type y / n"
                }
                FrontendMode::WaitingForApproval => "Waiting",
            }
        };

        Line::from(vec![
            Span::styled(format!("{mode_label} "), mode_style),
            Span::styled(status_text.to_string(), Style::default().fg(Color::Gray)),
            Span::raw("  "),
            Span::styled("·", Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(hint, Style::default().fg(Color::DarkGray)),
        ])
    }
}
