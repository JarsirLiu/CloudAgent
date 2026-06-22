use super::humanize_tool_label;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub(super) fn pretty_tool_title(label: &str) -> String {
    match label {
        "context" => "Context".to_string(),
        "conversation" => "conversation".to_string(),
        "reasoning" => "reasoning".to_string(),
        other => humanize_tool_label(other),
    }
}

pub(super) fn compact_inline_preview(input: &str, max_chars: usize) -> String {
    let trimmed = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

pub(super) fn is_generic_tool_group_summary(summary: &str) -> bool {
    matches!(
        summary.trim().to_ascii_lowercase().as_str(),
        "exploring workspace" | "running tool"
    )
}

pub(super) fn tint_all_style(style: Style) -> impl Fn(Line<'static>) -> Line<'static> {
    move |line| {
        let spans = line
            .spans
            .into_iter()
            .map(|span| Span::styled(span.content.into_owned(), style))
            .collect::<Vec<_>>();
        Line::from(spans)
    }
}

pub(super) fn tint_tail_style(style: Style) -> impl Fn(Line<'static>) -> Line<'static> {
    move |line| {
        let spans = line
            .spans
            .into_iter()
            .enumerate()
            .map(|(index, span)| {
                if index == 0 {
                    span
                } else {
                    Span::styled(span.content.into_owned(), style)
                }
            })
            .collect::<Vec<_>>();
        Line::from(spans)
    }
}
