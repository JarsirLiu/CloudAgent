use agent_protocol::FrontendMode;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

pub fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled(
        "-".repeat(width),
        Style::default().fg(Color::Rgb(40, 40, 50)),
    ))
}

pub fn status_line(
    mode: FrontendMode,
    status_text: &str,
    meta: &str,
    width: usize,
) -> Line<'static> {
    let (dot_color, mode_label, badge_bg) = match mode {
        FrontendMode::Idle => (Color::Rgb(80, 200, 120), "ready", Color::Rgb(18, 34, 24)),
        FrontendMode::Running => (Color::Rgb(100, 160, 255), "working", Color::Rgb(18, 28, 45)),
        FrontendMode::WaitingForServerRequest => {
            (Color::Rgb(255, 180, 50), "action", Color::Rgb(48, 34, 14))
        }
    };

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(
            format!(" state {mode_label} "),
            Style::default()
                .fg(dot_color)
                .bg(badge_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if !status_text.trim().is_empty() && !status_text.eq_ignore_ascii_case(mode_label) {
        spans.push(Span::styled(
            "  .  ",
            Style::default().fg(Color::Rgb(60, 60, 70)),
        ));
        spans.push(Span::styled(
            status_text.to_string(),
            Style::default().fg(Color::Rgb(140, 140, 155)),
        ));
    }
    if !meta.is_empty() {
        let current_width: usize = spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();
        let available_meta = width.saturating_sub(current_width + 5);
        if available_meta == 0 {
            return Line::from(spans);
        }
        let mut meta_text = String::new();
        let mut used = 0usize;
        for ch in meta.chars() {
            let w = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
            if used + w > available_meta {
                meta_text.push_str("...");
                break;
            }
            meta_text.push(ch);
            used += w;
        }
        spans.push(Span::styled(
            "  .  ",
            Style::default().fg(Color::Rgb(60, 60, 70)),
        ));
        spans.push(Span::styled(
            meta_text,
            Style::default().fg(Color::Rgb(95, 105, 120)),
        ));
    }
    Line::from(spans)
}

pub fn hint_line(mode: FrontendMode, width: usize) -> Line<'static> {
    let hint = match mode {
        FrontendMode::Idle => "  Enter submit  .  Ctrl+K interrupt  .  / commands",
        FrontendMode::Running => "  Ctrl+K interrupt the current turn",
        FrontendMode::WaitingForServerRequest => "  Enter submit  .  y approve  .  n deny",
    };
    let hint = truncate_single_line(hint, width.saturating_sub(1));
    Line::from(Span::styled(
        hint,
        Style::default().fg(Color::Rgb(62, 62, 78)),
    ))
}

fn truncate_single_line(input: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in input.chars() {
        let w = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if used + w > max_width.saturating_sub(3) {
            out.push_str("...");
            return out;
        }
        out.push(ch);
        used += w;
    }
    out
}
