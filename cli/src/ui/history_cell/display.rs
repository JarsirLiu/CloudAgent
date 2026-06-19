use super::humanize_tool_label;
use super::markdown;
use super::{
    HistoryCell, HistoryContent, HistoryFormat, HistoryKind, HistoryTone, ReasoningPresentation,
    ToolGroupCell,
};
use super::wrapping::{WrapOptions, word_wrap_text};
use crate::text_width::display_width;
use crate::ui::theme::{
    history_body_style, history_meta_marker_style, history_meta_style, history_more_style,
    history_notice_control_style, history_notice_error_style, history_notice_warning_style,
    history_rail_style, history_reasoning_style, history_strong_text_style,
    history_title_accent_style, history_title_style, history_tool_style, user_marker_style,
    user_message_style,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

pub(crate) fn render_cell_lines(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.kind() {
        HistoryKind::Message if cell.tone == HistoryTone::User => render_user(cell, width),
        HistoryKind::Message => render_agent(cell, width),
        HistoryKind::Reasoning => render_reasoning(cell, width),
        HistoryKind::Exploration => render_exploration(cell, width),
        HistoryKind::Command => render_command(cell, width),
        HistoryKind::Tool => render_tool(cell, width),
        HistoryKind::Notice => render_notice(cell, width),
    }
}

pub(crate) fn render_transcript_lines(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.kind() {
        HistoryKind::Message if cell.tone == HistoryTone::User => render_user(cell, width),
        HistoryKind::Message => render_agent_transcript(cell, width),
        HistoryKind::Reasoning => render_reasoning(cell, width),
        HistoryKind::Exploration => render_compact_transcript(cell, width, "◦"),
        HistoryKind::Command => render_compact_transcript(cell, width, "›"),
        HistoryKind::Tool => render_compact_transcript(cell, width, "•"),
        HistoryKind::Notice => render_notice_transcript(cell, width),
    }
}

pub(crate) fn render_live_transcript_lines(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.kind() {
        HistoryKind::Reasoning => render_reasoning_live(cell, width),
        _ => render_transcript_lines(cell, width),
    }
}

pub(crate) fn rendered_line_count(lines: &[Line<'static>], width: usize) -> usize {
    if lines.is_empty() {
        return 0;
    }
    Paragraph::new(Text::from(lines.to_vec()))
        .wrap(Wrap { trim: false })
        .line_count(width as u16)
}
fn render_user(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    if cell.body().trim().is_empty() {
        return Vec::new();
    }
    let inner = width.saturating_sub(2).max(8);
    let style = user_message_style();
    let text_style = history_body_style().add_modifier(Modifier::BOLD);
    word_wrap_text(cell.body(), WrapOptions::new(inner))
        .into_iter()
        .enumerate()
        .map(|(line_index, line)| {
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(if line_index == 0 {
                Span::styled("› ", user_marker_style())
            } else {
                Span::raw("  ")
            });
            spans.extend(
                line.spans
                    .into_iter()
                    .map(|span| Span::styled(span.content.into_owned(), text_style))
                    .collect::<Vec<_>>(),
            );
            apply_full_width_style(Line::from(spans), style, width)
        })
        .collect()
}

fn render_agent(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let inner = width.saturating_sub(2).max(8);
    let lines = match cell.format() {
        HistoryFormat::Markdown => markdown::render_markdown(cell.body(), inner),
        HistoryFormat::PlainText => markdown::render_plaintext(cell.body(), inner),
    };
    indent_agent_lines(lines)
}

fn render_agent_transcript(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let inner = width.saturating_sub(2).max(8);
    let lines = match cell.format() {
        HistoryFormat::Markdown => markdown::render_markdown(cell.body(), inner),
        HistoryFormat::PlainText => markdown::render_plaintext(cell.body(), inner),
    };
    indent_agent_lines(lines)
}

fn indent_agent_lines(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(Span::raw("  "));
            spans.extend(line.spans);
            Line::from(spans).style(line.style)
        })
        .collect()
}

fn render_reasoning(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let max_lines = if cell.is_expanded() { 24usize } else { 12usize };
    render_reasoning_with_limit(cell, width, Some(max_lines))
}

fn render_reasoning_live(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    render_reasoning_with_limit(cell, width, None)
}

fn render_reasoning_with_limit(
    cell: &HistoryCell,
    width: usize,
    max_lines: Option<usize>,
) -> Vec<Line<'static>> {
    let HistoryContent::Reasoning(reasoning) = &cell.content else {
        return Vec::new();
    };
    if reasoning.presentation == ReasoningPresentation::Summary {
        return render_reasoning_summary(cell, width);
    }
    let header = Line::from(vec![
        Span::raw("  "),
        Span::styled("≈ ", history_reasoning_style()),
        Span::styled(
            if cell.label().is_empty() {
                "Reasoning".to_string()
            } else {
                cell.label().to_string()
            },
            history_strong_text_style(),
        ),
    ]);
    let subsequent_indent = Line::from(vec![
        Span::raw("    "),
        Span::styled("│ ", history_rail_style()),
    ]);

    let mut out = vec![header];
    let paragraphs = reasoning_paragraphs(cell.body());
    let paragraph_count = paragraphs.len();
    for (index, paragraph) in paragraphs.into_iter().enumerate() {
        let lines = word_wrap_text(
            &paragraph,
            WrapOptions::new(width)
                .initial_indent(subsequent_indent.clone())
                .subsequent_indent(subsequent_indent.clone()),
        );
        out.extend(lines);
        if index + 1 < paragraph_count && !out.is_empty() {
            // Preserve paragraph spacing while keeping the same continuous reasoning gutter.
            out.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", history_rail_style()),
            ]));
        }
    }
    while out.last().is_some_and(|line| {
        line.spans
            .iter()
            .all(|span| span.content.as_ref().trim().is_empty())
    }) {
        out.pop();
    }
    let Some(max_lines) = max_lines else {
        return out;
    };
    let hidden_lines = out.len().saturating_sub(max_lines);
    if hidden_lines == 0 {
        return out;
    }
    let mut kept = out.into_iter().take(max_lines).collect::<Vec<_>>();
    kept.push(Line::from(vec![
        Span::raw("    "),
        Span::styled("│ ", history_rail_style()),
        Span::styled(
            format!("… +{} lines", hidden_lines),
            history_more_style(),
        ),
    ]));
    kept
}

fn render_reasoning_summary(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    render_reasoning_lines(
        cell.body(),
        width,
            history_more_style().add_modifier(Modifier::ITALIC),
    )
}

fn render_reasoning_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let paragraphs = reasoning_paragraphs(text);
    let mut lines = Vec::new();

    for (index, paragraph) in paragraphs.into_iter().enumerate() {
        let initial_indent = if index == 0 {
            Line::from(vec![Span::styled(
                "• ",
                history_reasoning_style(),
            )])
        } else {
            Line::from("  ")
        };
        let wrapped = word_wrap_text(
            &paragraph,
            WrapOptions::new(width)
                .initial_indent(initial_indent)
                .subsequent_indent(Line::from("  ")),
        )
        .into_iter()
        .map(|mut line| {
            line.spans = line
                .spans
                .into_iter()
                .map(|span| span.patch_style(style))
                .collect();
            line
        })
        .collect::<Vec<_>>();
        lines.extend(wrapped);
    }

    lines
}

fn reasoning_paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
            continue;
        }
        current.push(line.trim().to_string());
    }

    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }

    if paragraphs.is_empty() && !text.trim().is_empty() {
        paragraphs.push(text.trim().to_string());
    }

    paragraphs
}

fn render_exploration(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
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
        Span::styled("◦ ", history_tool_style()),
        Span::styled(title, history_title_style()),
    ])];

    lines.extend(
        word_wrap_text(
            cell.body(),
                WrapOptions::new(width)
                    .initial_indent(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("│ ", history_rail_style()),
                    ]))
                    .subsequent_indent(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("│ ", history_rail_style()),
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

fn render_command(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        "Command".to_string()
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("› ", history_tool_style()),
        Span::styled(title, history_title_style()),
    ])];

    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from("    "))
                .subsequent_indent(Line::from("    ")),
        )
        .into_iter()
        .map(tint_all_style(history_body_style())),
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
                                Span::styled("↳ ", history_rail_style()),
                            ]))
                            .subsequent_indent(Line::from("      ")),
                    )
                })
            .collect::<Vec<_>>();
        let max_lines = if cell.is_expanded() { 24usize } else { 5usize };
        let display_lines: Vec<Line<'static>> = if raw_lines.len() <= max_lines {
            raw_lines
        } else {
            let mut kept = raw_lines.into_iter().take(max_lines).collect::<Vec<_>>();
            kept.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("↳ ", history_rail_style()),
                Span::styled(
                    format!(
                        "… +{} lines",
                        detail.lines().count().saturating_sub(max_lines)
                    ),
                    history_more_style(),
                ),
            ]));
            kept
        };
        lines.extend(display_lines.into_iter().map(tint_tail_style(history_more_style())));
    }

    lines
}

fn render_tool(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match &cell.content {
        HistoryContent::ToolGroup(group) => render_tool_group(cell, group, width),
        _ => render_tool_like(cell, width, history_tool_style(), "•"),
    }
}

fn apply_full_width_style(mut line: Line<'static>, style: Style, width: usize) -> Line<'static> {
    let padding = width.saturating_sub(line_display_width(&line));
    if padding > 0 {
        line.spans.push(Span::styled(" ".repeat(padding), style));
    }
    line.style = line.style.patch(style);
    for span in &mut line.spans {
        span.style = span.style.patch(style);
    }
    line
}

fn line_display_width(line: &Line<'_>) -> usize {
    line.spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

fn render_tool_group(
    cell: &HistoryCell,
    group: &ToolGroupCell,
    width: usize,
) -> Vec<Line<'static>> {
    let title = pretty_tool_title(&group.label);
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("• ", history_tool_style()),
        Span::styled(title, history_title_accent_style().add_modifier(Modifier::BOLD)),
    ])];

    if !is_generic_tool_group_summary(&group.summary) {
        lines.extend(
            word_wrap_text(
                &group.summary,
                WrapOptions::new(width)
                    .initial_indent(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("│ ", history_rail_style()),
                    ]))
                    .subsequent_indent(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("│ ", history_rail_style()),
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
                Span::styled("│ ", history_rail_style()),
                Span::styled(
                    if index + 1 == preview_count && group.children.len() == 1 {
                        "└ "
                    } else {
                        "├ "
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
                Span::styled("│ ", history_rail_style()),
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
    let branch = if is_last { "└ " } else { "├ " };
    let rail = if is_last { "  " } else { "│ " };
    let title = if cell.label().is_empty() {
        "Step".to_string()
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::raw("    "),
        Span::styled(branch, history_rail_style()),
        Span::styled(title, history_title_accent_style().add_modifier(Modifier::BOLD)),
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
                            Span::styled("↳ ", history_rail_style()),
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
        lines.extend(display_lines.into_iter().map(tint_tail_style(history_more_style())));
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
        Span::styled(title, history_title_accent_style().add_modifier(Modifier::BOLD)),
    ])];
    let wrapped = word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", history_rail_style()),
            ]))
            .subsequent_indent(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", history_rail_style()),
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
            Span::styled("│ ", history_rail_style()),
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
                                Span::styled("└ ", history_rail_style()),
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
        lines.extend(display_lines.into_iter().map(tint_tail_style(history_more_style())));
    }
    lines
}

fn render_meta(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from(vec![Span::styled(
                "· ",
                history_meta_marker_style(),
            )]))
            .subsequent_indent(Line::from("  ")),
    )
    .into_iter()
    .map(tint_tail_style(history_meta_style()))
    .collect()
}

fn render_notice(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.tone {
        HistoryTone::Warning => render_tool_like(cell, width, history_notice_warning_style(), "◆"),
        HistoryTone::Error => render_tool_like(cell, width, history_notice_error_style(), "◆"),
        HistoryTone::Meta => render_meta(cell, width),
        _ => render_tool_like(cell, width, history_tool_style(), "•"),
    }
}

fn render_notice_transcript(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let accent = match cell.tone {
        HistoryTone::Error => history_notice_error_style(),
        HistoryTone::Warning => history_notice_warning_style(),
        HistoryTone::Control => history_notice_control_style(),
        _ => history_notice_control_style(),
    };
    let mut title = cell.label().to_string();
    if title.is_empty() {
        title = "Notice".to_string();
    }
    let body_lines = word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from("  "))
            .subsequent_indent(Line::from("  ")),
    )
    .into_iter()
    .map(tint_tail_style(history_body_style()))
    .collect::<Vec<_>>();

    let mut lines = vec![Line::from(vec![
        Span::styled("• ", accent),
        Span::styled(title, history_title_accent_style().add_modifier(Modifier::BOLD)),
    ])];
    lines.extend(body_lines);
    lines
}

fn render_compact_transcript(cell: &HistoryCell, width: usize, bullet: &str) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        match cell.kind() {
            HistoryKind::Exploration => "Explored workspace".to_string(),
            HistoryKind::Command => "Command".to_string(),
            HistoryKind::Tool => "Tool".to_string(),
            _ => "Step".to_string(),
        }
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{bullet} "), history_tool_style()),
        Span::styled(title, history_title_accent_style().add_modifier(Modifier::BOLD)),
    ])];
    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from("  "))
                .subsequent_indent(Line::from("  ")),
        )
        .into_iter()
        .map(tint_tail_style(history_body_style())),
    );
    lines
}

fn pretty_tool_title(label: &str) -> String {
    match label {
        "context" => "Context".to_string(),
        "conversation" => "conversation".to_string(),
        "reasoning" => "reasoning".to_string(),
        other => humanize_tool_label(other),
    }
}

fn compact_inline_preview(input: &str, max_chars: usize) -> String {
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

fn is_generic_tool_group_summary(summary: &str) -> bool {
    matches!(
        summary.trim().to_ascii_lowercase().as_str(),
        "exploring workspace" | "running tool"
    )
}

fn tint_all_style(style: Style) -> impl Fn(Line<'static>) -> Line<'static> {
    move |line| {
        let spans = line
            .spans
            .into_iter()
            .map(|span| Span::styled(span.content.into_owned(), style))
            .collect::<Vec<_>>();
        Line::from(spans)
    }
}

fn tint_tail_style(style: Style) -> impl Fn(Line<'static>) -> Line<'static> {
    move |line| {
        let spans = line
            .spans
            .into_iter()
            .enumerate()
            .map(|(index, span)| if index == 0 { span } else { Span::styled(span.content.into_owned(), style) })
            .collect::<Vec<_>>();
        Line::from(spans)
    }
}
