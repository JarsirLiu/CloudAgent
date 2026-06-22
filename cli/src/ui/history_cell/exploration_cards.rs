use super::HistoryCell;
use super::display_common::{tint_all_style, tint_tail_style};
use super::wrapping::{WrapOptions, word_wrap_text};
use crate::ui::theme::{
    history_body_style, history_more_style, history_rail_style, history_title_style,
    history_tool_style,
};
use ratatui::text::{Line, Span};

pub(super) fn render_exploration(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        "Explored workspace".to_string()
    } else {
        cell.label().to_string()
    };
    let details = cell
        .aggregate()
        .map(|aggregate| aggregate.details.as_slice())
        .unwrap_or(&[]);
    let max_details = if cell.is_expanded() { 8 } else { 2 };
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("◼", history_tool_style()),
        Span::styled(title, history_title_style()),
    ])];

    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("┆", history_rail_style()),
                ]))
                .subsequent_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("┆", history_rail_style()),
                ])),
        )
        .into_iter()
        .map(tint_tail_style(history_body_style())),
    );

    for (index, detail) in details.iter().take(max_details).enumerate() {
        let indent = if index == 0 { "    └ " } else { "      " };
        lines.extend(
            word_wrap_text(
                detail,
                WrapOptions::new(width)
                    .initial_indent(Line::from(indent))
                    .subsequent_indent(Line::from("    ")),
            )
            .into_iter()
            .map(tint_all_style(history_body_style())),
        );
    }

    if details.len() > max_details {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                format!("… +{} more", details.len().saturating_sub(max_details)),
                history_more_style(),
            ),
        ]));
    }

    lines
}
