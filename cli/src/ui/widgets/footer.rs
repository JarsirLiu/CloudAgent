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
    runtime_hint: Option<&str>,
    meta: &str,
    width: usize,
) -> Line<'static> {
    if mode != FrontendMode::Idle {
        return running_status_line(status_text, runtime_hint, meta, width);
    }

    let (dot_color, mode_label, badge_bg) = match mode {
        FrontendMode::Idle => (Color::Rgb(80, 200, 120), "ready", Color::Rgb(18, 34, 24)),
        FrontendMode::Running => (Color::Rgb(100, 160, 255), "working", Color::Rgb(18, 28, 45)),
        FrontendMode::WaitingForServerRequest => {
            (Color::Rgb(255, 180, 50), "action", Color::Rgb(48, 34, 14))
        }
    };

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            format!(" {mode_label} "),
            Style::default()
                .fg(dot_color)
                .bg(badge_bg)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if !status_text.trim().is_empty() && !status_text.eq_ignore_ascii_case(mode_label) {
        spans.push(Span::styled(
            " · ",
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
        let separator_width = 3usize;
        let terminal_wrap_guard = 1usize;
        let available_meta =
            width.saturating_sub(current_width + separator_width + terminal_wrap_guard);
        if available_meta == 0 {
            return Line::from(spans);
        }
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Rgb(60, 60, 70)),
        ));
        spans.push(Span::styled(
            truncate_single_line(meta, available_meta),
            Style::default().fg(Color::Rgb(95, 105, 120)),
        ));
    }
    Line::from(spans)
}

fn running_status_line(
    status_text: &str,
    runtime_hint: Option<&str>,
    meta: &str,
    width: usize,
) -> Line<'static> {
    let header = if status_text.trim().is_empty() {
        "Working"
    } else {
        status_text
    };
    let mut spans = vec![
        Span::styled("• ", Style::default().fg(Color::Rgb(100, 160, 255))),
        Span::styled(
            header.to_string(),
            Style::default()
                .fg(Color::Rgb(210, 215, 225))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(runtime_hint) = runtime_hint.filter(|hint| !hint.trim().is_empty()) {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("({runtime_hint})"),
            Style::default().fg(Color::Rgb(132, 138, 150)),
        ));
    }
    if !meta.is_empty() {
        let current_width: usize = spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();
        let separator_width = 3usize;
        let terminal_wrap_guard = 1usize;
        let available_meta =
            width.saturating_sub(current_width + separator_width + terminal_wrap_guard);
        if available_meta > 0 {
            spans.push(Span::styled(
                " · ",
                Style::default().fg(Color::Rgb(60, 60, 70)),
            ));
            spans.push(Span::styled(
                truncate_single_line(meta, available_meta),
                Style::default().fg(Color::Rgb(95, 105, 120)),
            ));
        }
    }
    Line::from(spans)
}

pub fn hint_line(mode: FrontendMode, width: usize, meta: &str) -> Line<'static> {
    let base = match mode {
        FrontendMode::Idle => "  Enter submit  .  Ctrl+D exit  .  / commands",
        FrontendMode::Running => "  Esc interrupt the current turn",
        FrontendMode::WaitingForServerRequest => "  Enter submit  .  y approve  .  n deny",
    };
    let hint = if mode == FrontendMode::Idle && !meta.trim().is_empty() {
        format!("{base}  .  {meta}")
    } else {
        base.to_string()
    };
    let hint = truncate_single_line(&hint, width.saturating_sub(1));
    Line::from(Span::styled(
        hint,
        Style::default().fg(Color::Rgb(62, 62, 78)),
    ))
}

fn truncate_single_line(input: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(input) <= max_width {
        return input.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut out = String::new();
    let mut used = 0usize;
    let content_limit = max_width - 3;
    for ch in input.chars() {
        let w = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]));
        if used + w > content_limit {
            out.push_str("...");
            return out;
        }
        out.push(ch);
        used += w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_width(line: &Line<'_>) -> usize {
        line.spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum()
    }

    #[test]
    fn status_line_never_exceeds_available_width() {
        let line = status_line(
            FrontendMode::Running,
            "Request approved:",
            Some("0s • esc to interrupt"),
            "in 1.3k tokens · out 93 tokens · cached 0 tokens · total 1.4k tokens",
            58,
        );

        assert!(line_width(&line) < 58);
    }

    #[test]
    fn truncate_single_line_counts_ellipsis_inside_budget() {
        let truncated = truncate_single_line("abcdef", 5);

        assert_eq!(truncated, "ab...");
        assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 5);
    }
}
