use agent_protocol::FrontendMode;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::history_cell::shimmer_spans;

pub fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(Color::Rgb(40, 40, 50)),
    ))
}

pub fn status_line(mode: FrontendMode, status_text: &str, meta: &str) -> Line<'static> {
    let (dot_color, mode_label, badge_bg) = match mode {
        FrontendMode::Idle => (Color::Rgb(80, 200, 120), "IDLE", Color::Rgb(18, 34, 24)),
        FrontendMode::Running => (Color::Rgb(100, 160, 255), "WORKING", Color::Rgb(18, 28, 45)),
        FrontendMode::WaitingForApproval => {
            (Color::Rgb(255, 180, 50), "APPROVAL", Color::Rgb(48, 34, 14))
        }
    };

    let status_spans = if mode == FrontendMode::Running {
        shimmer_spans(status_text)
    } else {
        vec![Span::styled(
            status_text.to_string(),
            Style::default().fg(Color::Rgb(140, 140, 155)),
        )]
    };

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(
            format!(" {mode_label} "),
            Style::default()
                .fg(dot_color)
                .bg(badge_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(Color::Rgb(60, 60, 70))),
    ];
    spans.extend(status_spans);
    if !meta.is_empty() {
        spans.push(Span::styled(
            "  ·  ",
            Style::default().fg(Color::Rgb(60, 60, 70)),
        ));
        spans.push(Span::styled(
            meta.to_string(),
            Style::default().fg(Color::Rgb(95, 105, 120)),
        ));
    }
    Line::from(spans)
}

pub fn hint_line(mode: FrontendMode) -> Line<'static> {
    let hint = match mode {
        FrontendMode::Idle => {
            "  Enter submit  ·  Ctrl+F search  ·  [ ] tool nav  ·  o toggle tool  ·  O all tools"
        }
        FrontendMode::Running => "  Ctrl+K interrupt the current turn",
        FrontendMode::WaitingForApproval => "  Enter submit  ·  y approve  ·  n deny",
    };
    Line::from(Span::styled(
        hint,
        Style::default().fg(Color::Rgb(62, 62, 78)),
    ))
}
