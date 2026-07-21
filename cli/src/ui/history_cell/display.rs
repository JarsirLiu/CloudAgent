use super::display_cards;
use super::markdown;
use super::wrapping::{WrapOptions, word_wrap_text};
use super::{
    HistoryCell, HistoryContent, HistoryFormat, HistoryKind, HistoryTone, ReasoningPresentation,
};
use crate::text_width::display_width;
use crate::ui::theme::{
    history_body_style, history_rail_style, history_reasoning_style, history_strong_text_style,
    user_marker_style, user_message_style,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

pub(crate) fn render_cell_lines(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.kind() {
        HistoryKind::Message if cell.tone == HistoryTone::User => render_user(cell, width),
        HistoryKind::Message => render_agent(cell, width),
        HistoryKind::Reasoning => render_reasoning(cell, width),
        HistoryKind::Exploration => display_cards::render_exploration(cell, width),
        HistoryKind::Search => display_cards::render_search(cell, width),
        HistoryKind::Command => display_cards::render_command(cell, width),
        HistoryKind::Patch => display_cards::render_patch(cell, width),
        HistoryKind::Tool => display_cards::render_tool(cell, width),
        HistoryKind::Notice => display_cards::render_notice(cell, width),
    }
}

pub(crate) fn render_transcript_lines(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.kind() {
        HistoryKind::Message if cell.tone == HistoryTone::User => render_user(cell, width),
        HistoryKind::Message => render_agent_transcript(cell, width),
        HistoryKind::Reasoning => render_reasoning(cell, width),
        HistoryKind::Exploration => display_cards::render_compact_transcript(cell, width, "◦"),
        HistoryKind::Search => display_cards::render_compact_transcript(cell, width, "◦"),
        HistoryKind::Command => display_cards::render_compact_transcript(cell, width, "›"),
        HistoryKind::Patch => display_cards::render_compact_transcript(cell, width, "◦"),
        HistoryKind::Tool => display_cards::render_compact_transcript(cell, width, "•"),
        HistoryKind::Notice => display_cards::render_notice_transcript(cell, width),
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
            crate::ui::theme::history_more_style(),
        ),
    ]));
    kept
}

fn render_reasoning_summary(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    render_reasoning_lines(
        cell.body(),
        width,
        crate::ui::theme::history_more_style().add_modifier(Modifier::ITALIC),
    )
}

fn render_reasoning_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let paragraphs = reasoning_paragraphs(text);
    let mut lines = Vec::new();

    for (index, paragraph) in paragraphs.into_iter().enumerate() {
        let initial_indent = if index == 0 {
            Line::from(vec![Span::styled("• ", history_reasoning_style())])
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
