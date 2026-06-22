use super::card_layout::{truncate_lines, wrap_multiline_detail};
use ratatui::text::{Line, Span};

fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

#[test]
fn wrap_multiline_detail_keeps_each_input_line_visible() {
    let (initial, subsequent) = (
        Line::from(vec![Span::raw("  ")]),
        Line::from(vec![Span::raw("    ")]),
    );
    let lines = wrap_multiline_detail(Some("one\ntwo"), 20, initial, subsequent);

    let rendered = lines.iter().map(line_text).collect::<Vec<_>>();
    assert!(rendered.iter().any(|line| line.contains("one")));
    assert!(rendered.iter().any(|line| line.contains("two")));
}

#[test]
fn truncate_lines_adds_overflow_line() {
    let lines = vec![
        Line::from("a"),
        Line::from("b"),
        Line::from("c"),
        Line::from("d"),
    ];

    let rendered = truncate_lines(lines, 2, |hidden| Line::from(format!("+{hidden} more")));
    let text = rendered
        .into_iter()
        .map(|line| line_text(&line))
        .collect::<Vec<_>>();

    assert_eq!(text, vec!["a", "b", "+2 more"]);
}
