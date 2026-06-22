use super::display_common::{
    compact_inline_preview, is_generic_tool_group_summary, pretty_tool_title, tint_tail_style,
};
use super::wrapping::{WrapOptions, word_wrap_text};
use super::{HistoryCell, HistoryContent, ToolGroupCell};
use crate::ui::theme::{
    history_body_style, history_more_style, history_rail_style, history_title_accent_style,
    history_tool_style,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub(super) fn render_command(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    render_tool_like(cell, width, history_tool_style(), "▸")
}

pub(super) fn render_patch(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    render_tool_like(cell, width, history_title_accent_style(), "◼")
}

pub(super) fn render_tool(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match &cell.content {
        HistoryContent::ToolGroup(group) => render_tool_group(cell, group, width),
        _ => render_tool_like(cell, width, history_tool_style(), "▸"),
    }
}

pub(super) fn render_tool_group(
    cell: &HistoryCell,
    group: &ToolGroupCell,
    width: usize,
) -> Vec<Line<'static>> {
    let title = pretty_tool_title(&group.label);
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("▸", history_tool_style()),
        Span::styled(
            title,
            history_title_accent_style().add_modifier(Modifier::BOLD),
        ),
    ])];

    if !is_generic_tool_group_summary(&group.summary) {
        lines.extend(
            word_wrap_text(
                &group.summary,
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
    }

    if !cell.is_expanded() {
        let preview_count = group.children.len().min(2);
        for (index, child) in group.children.iter().take(preview_count).enumerate() {
            let step_title = if child.label().is_empty() {
                "Step".to_string()
            } else {
                child.label().to_string()
            };
            let preview_body = compact_inline_preview(child.body(), 72);
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("┆", history_rail_style()),
                Span::styled(
                    if index + 1 == preview_count && group.children.len() == 1 {
                        "└"
                    } else {
                        "├"
                    },
                    history_rail_style(),
                ),
                Span::styled(
                    step_title,
                    history_title_accent_style().add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(preview_body, history_body_style()),
            ]));
        }
        let hidden_count = group.children.len().saturating_sub(preview_count);
        if hidden_count > 0 {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("┆", history_rail_style()),
                Span::styled(
                    format!(
                        "{} more step{}",
                        hidden_count,
                        if hidden_count == 1 { "" } else { "s" }
                    ),
                    history_more_style(),
                ),
            ]));
        }
        return lines;
    }

    for (index, child) in group.children.iter().enumerate() {
        let is_last = index + 1 == group.children.len();
        lines.extend(render_tool_group_child(child, width, is_last));
    }

    lines
}

fn render_tool_group_child(cell: &HistoryCell, width: usize, is_last: bool) -> Vec<Line<'static>> {
    let branch = if is_last { "└" } else { "├" };
    let rail = if is_last { "  " } else { "│" };
    let title = if cell.label().is_empty() {
        "Step".to_string()
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::raw("    "),
        Span::styled(branch, history_rail_style()),
        Span::styled(
            title,
            history_title_accent_style().add_modifier(Modifier::BOLD),
        ),
    ])];

    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(rail, history_rail_style()),
                ]))
                .subsequent_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(rail, history_rail_style()),
                ])),
        )
        .into_iter()
        .map(tint_tail_style(history_body_style())),
    );

    if let Some(detail) = cell.detail() {
        let raw_lines = detail
            .lines()
            .flat_map(|line| {
                word_wrap_text(
                    line,
                    WrapOptions::new(width)
                        .initial_indent(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(rail, history_rail_style()),
                            Span::styled("↳", history_rail_style()),
                        ]))
                        .subsequent_indent(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(rail, history_rail_style()),
                            Span::raw("  "),
                        ])),
                )
            })
            .collect::<Vec<_>>();
        let max_lines = if cell.is_expanded() { 12usize } else { 3usize };
        let display_lines: Vec<Line<'static>> = if raw_lines.len() <= max_lines {
            raw_lines
        } else {
            let mut kept = raw_lines.into_iter().take(max_lines).collect::<Vec<_>>();
            kept.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(rail, history_rail_style()),
                Span::styled(
                    format!(
                        "… +{} more lines",
                        detail.lines().count().saturating_sub(max_lines)
                    ),
                    history_more_style(),
                ),
            ]));
            kept
        };
        lines.extend(
            display_lines
                .into_iter()
                .map(tint_tail_style(history_more_style())),
        );
    }

    lines
}

fn render_tool_like(
    cell: &HistoryCell,
    width: usize,
    accent: Style,
    dot: &str,
) -> Vec<Line<'static>> {
    let title = pretty_tool_title(cell.label());
    let title = if cell.repeat_count() > 1 {
        format!("{title} x{}", cell.repeat_count())
    } else {
        title
    };
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{dot} "), accent),
        Span::styled(
            title,
            history_title_accent_style().add_modifier(Modifier::BOLD),
        ),
    ])];
    let wrapped = word_wrap_text(
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
    );
    let max_lines = if cell.is_expanded() { 24usize } else { 2usize };
    let mut output_lines: Vec<Line<'static>> = Vec::new();
    if wrapped.len() <= max_lines {
        output_lines.extend(wrapped);
    } else {
        output_lines.extend(wrapped.iter().take(max_lines).cloned());
        output_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("┆", history_rail_style()),
            Span::styled(
                format!("… +{} lines", wrapped.len().saturating_sub(max_lines)),
                history_more_style(),
            ),
        ]));
    }
    lines.extend(
        output_lines
            .into_iter()
            .filter(|line| !line.spans.is_empty())
            .map(tint_tail_style(history_more_style())),
    );

    if let Some(detail) = cell.detail() {
        let raw_lines = detail
            .lines()
            .flat_map(|line| {
                word_wrap_text(
                    line,
                    WrapOptions::new(width)
                        .initial_indent(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("┆", history_rail_style()),
                        ]))
                        .subsequent_indent(Line::from(vec![
                            Span::raw("      "),
                            Span::styled("  ", history_rail_style()),
                        ])),
                )
            })
            .collect::<Vec<_>>();
        let max_detail_lines = if cell.is_expanded() { 12usize } else { 3usize };
        let display_lines: Vec<Line<'static>> = if raw_lines.len() <= max_detail_lines {
            raw_lines
        } else {
            let mut kept = raw_lines
                .into_iter()
                .take(max_detail_lines)
                .collect::<Vec<_>>();
            kept.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(
                    format!(
                        "… +{} more lines",
                        detail.lines().count().saturating_sub(max_detail_lines)
                    ),
                    history_more_style(),
                ),
            ]));
            kept
        };
        lines.extend(
            display_lines
                .into_iter()
                .map(tint_tail_style(history_more_style())),
        );
    }
    lines
}
