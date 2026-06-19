use agent_protocol::FrontendMode;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;
use crate::ui::theme::{hint_style, muted_style, status_divider_style, status_mode_style, title_style};

pub fn divider_line(width: usize) -> Line<'static> {
    Line::from(Span::styled("-".repeat(width), status_divider_style()))
}

pub fn status_line(
    mode: FrontendMode,
    indicator: Option<&str>,
    status_text: &str,
    runtime_hint: Option<&str>,
    meta: &str,
    width: usize,
) -> Line<'static> {
    if mode != FrontendMode::Idle {
        return running_status_line(indicator, status_text, runtime_hint, meta, width);
    }

    let (mode_label, badge_style) = match mode {
        FrontendMode::Idle => ("ready", status_mode_style(mode)),
        FrontendMode::Running => ("working", status_mode_style(mode)),
        FrontendMode::WaitingForServerRequest => ("action", status_mode_style(mode)),
    };

    let mut spans = vec![
        Span::raw(" "),
        Span::styled(format!(" {mode_label} "), badge_style),
    ];

    if !status_text.trim().is_empty() && !status_text.eq_ignore_ascii_case(mode_label) {
        spans.push(Span::styled(" · ", status_divider_style()));
        spans.push(Span::styled(status_text.to_string(), muted_style()));
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
        spans.push(Span::styled(" · ", status_divider_style()));
        spans.push(Span::styled(truncate_single_line(meta, available_meta), hint_style()));
    }

    Line::from(spans)
}

fn running_status_line(
    indicator: Option<&str>,
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
        Span::styled(
        format!("{} ", indicator.unwrap_or("•")),
            status_mode_style(FrontendMode::Running),
        ),
        Span::styled(
            header.to_string(),
            title_style(),
        ),
    ];

    if let Some(runtime_hint) = runtime_hint
        .map(|hint| hint.split('·').next().unwrap_or(hint).trim())
        .filter(|hint| !hint.is_empty())
    {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("({runtime_hint})"),
            hint_style(),
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
        if available_meta != 0 {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                truncate_single_line(meta, available_meta),
                hint_style(),
            ));
        }
    }

    Line::from(spans)
}

pub fn hint_line(mode: FrontendMode, width: usize, meta: &str) -> Line<'static> {
    let base = match mode {
        FrontendMode::Idle => "  Enter submit  .  Ctrl+D exit  .  / commands",
        FrontendMode::Running => "  Esc to interrupt",
        FrontendMode::WaitingForServerRequest => "  Enter submit  .  y approve  .  n deny",
    };
    let hint = if mode == FrontendMode::Idle && !meta.trim().is_empty() {
        format!("{base}  .  {meta}")
    } else {
        base.to_string()
    };
    let hint = truncate_single_line(&hint, width.saturating_sub(1));
    Line::from(Span::styled(hint, hint_style()))
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
            Some("⠋"),
            "Request approved:",
            Some("0s · esc to interrupt"),
            "in 1.3k tokens · out 93 tokens · cached 0 tokens · total 1.4k tokens",
            58,
        );

        assert!(line_width(&line) < 58);
    }

    #[test]
    fn running_status_line_only_shows_short_runtime_hint() {
        let line = status_line(
            FrontendMode::Running,
            Some("⠋"),
            "Working",
            Some("15s · esc to interrupt"),
            "in 7.7k tokens · out 35 tokens · cached 0 tokens · total 7.7k tokens · context 3%",
            80,
        );

        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("(15s)"));
        assert!(!rendered.contains("esc to interrupt"));
        assert!(rendered.contains("tokens"));
    }

    #[test]
    fn truncate_single_line_counts_ellipsis_inside_budget() {
        let truncated = truncate_single_line("abcdef", 5);

        assert_eq!(truncated, "ab...");
        assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 5);
    }
}
