use super::wrapping::{WrapOptions, word_wrap_text};
use ratatui::text::Line;

pub(crate) fn wrap_block(
    text: &str,
    width: usize,
    initial_indent: Line<'static>,
    subsequent_indent: Line<'static>,
) -> Vec<Line<'static>> {
    word_wrap_text(
        text,
        WrapOptions::new(width)
            .initial_indent(initial_indent)
            .subsequent_indent(subsequent_indent),
    )
}

pub(crate) fn wrap_multiline_detail(
    detail: Option<&str>,
    width: usize,
    initial_indent: Line<'static>,
    subsequent_indent: Line<'static>,
) -> Vec<Line<'static>> {
    detail
        .into_iter()
        .flat_map(|text| {
            text.lines().flat_map(|line| {
                wrap_block(
                    line,
                    width,
                    initial_indent.clone(),
                    subsequent_indent.clone(),
                )
            })
        })
        .collect()
}

pub(crate) fn truncate_lines(
    lines: Vec<Line<'static>>,
    max_lines: usize,
    overflow_line: impl FnOnce(usize) -> Line<'static>,
) -> Vec<Line<'static>> {
    if lines.len() <= max_lines {
        return lines;
    }

    let hidden = lines.len().saturating_sub(max_lines);
    let mut kept = lines.into_iter().take(max_lines).collect::<Vec<_>>();
    kept.push(overflow_line(hidden));
    kept
}
