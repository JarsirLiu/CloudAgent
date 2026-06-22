use super::HistoryCell;
use super::card_layout::{truncate_lines, wrap_multiline_detail};
use super::display_common::tint_all_style;
use super::wrapping::{WrapOptions, word_wrap_text};
use crate::ui::theme::{
    history_body_style, history_more_style, history_rail_style, history_search_style,
    history_title_accent_style,
};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

pub(crate) fn render_search(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        "Search".to_string()
    } else {
        cell.label().to_string()
    };
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("◦ ", history_search_style()),
        Span::styled(
            title,
            history_title_accent_style().add_modifier(Modifier::BOLD),
        ),
    ])];

    let body_lines = word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from(vec![
                Span::raw("    "),
                Span::styled("╰─ ", history_rail_style()),
            ]))
            .subsequent_indent(Line::from(vec![
                Span::raw("    "),
                Span::styled("╰─ ", history_rail_style()),
            ])),
    );
    let max_lines = if cell.is_expanded() { 6usize } else { 2usize };
    if body_lines.len() <= max_lines {
        lines.extend(
            body_lines
                .into_iter()
                .map(tint_all_style(history_body_style())),
        );
    } else {
        lines.extend(
            body_lines
                .iter()
                .take(max_lines)
                .cloned()
                .map(tint_all_style(history_body_style())),
        );
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("╰─ ", history_rail_style()),
            Span::styled(
                format!("... +{} lines", body_lines.len().saturating_sub(max_lines)),
                history_more_style(),
            ),
        ]));
    }

    if let Some(detail) = cell.detail() {
        let detail_lines = wrap_multiline_detail(
            Some(detail),
            width,
            Line::from(vec![
                Span::raw("    "),
                Span::styled("╰─ ", history_rail_style()),
                Span::styled("↳ ", history_rail_style()),
            ]),
            Line::from(vec![
                Span::raw("    "),
                Span::styled("╰─ ", history_rail_style()),
                Span::raw("  "),
            ]),
        );
        let max_detail_lines = if cell.is_expanded() { 8usize } else { 3usize };
        let detail_lines = truncate_lines(detail_lines, max_detail_lines, |hidden| {
            Line::from(vec![
                Span::raw("    "),
                Span::styled("╰─ ", history_rail_style()),
                Span::styled(format!("... +{hidden} more lines"), history_more_style()),
            ])
        });
        lines.extend(
            detail_lines
                .into_iter()
                .map(tint_all_style(history_more_style())),
        );
    }

    lines
}
