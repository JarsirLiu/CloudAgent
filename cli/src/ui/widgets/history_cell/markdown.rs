use super::wrapping::{WrapOptions, word_wrap_spans, word_wrap_text};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use crate::text_width::display_width;

pub(super) fn render_markdown(input: &str, width: usize) -> Vec<Line<'static>> {
    let input = normalize_markdown_indentation(input);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(&input, opts);

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default().fg(Color::Rgb(200, 200, 210))];
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_indent = String::new();
    let mut code_buf = String::new();
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut line_prefix = String::new();
    let mut heading_prefix = String::new();
    let mut in_heading = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row: Vec<String> = Vec::new();
    let mut quote_depth = 0usize;
    let mut link_stack: Vec<String> = Vec::new();

    let flush =
        |current: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>, w: usize, prefix: &str| {
            if !current.is_empty() {
                push_wrapped_spans(current, out, w, prefix);
                current.clear();
            }
        };

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                in_code_block = true;
                code_lang = match &kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    _ => String::new(),
                };
                code_indent = match &kind {
                    CodeBlockKind::Fenced(_) => String::new(),
                    CodeBlockKind::Indented => "    ".to_string(),
                };
                code_buf.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                for line in code_buf.lines() {
                    let mut spans = Vec::new();
                    if !code_indent.is_empty() {
                        spans.push(Span::raw(code_indent.clone()));
                    }
                    spans.extend(highlight_code_line(line, &code_lang));
                    out.push(Line::from(spans));
                }
                code_indent.clear();
                out.push(Line::raw(""));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                in_heading = true;
                heading_prefix = match level {
                    pulldown_cmark::HeadingLevel::H1 => "# ".to_string(),
                    pulldown_cmark::HeadingLevel::H2 => "## ".to_string(),
                    pulldown_cmark::HeadingLevel::H3 => "### ".to_string(),
                    pulldown_cmark::HeadingLevel::H4
                    | pulldown_cmark::HeadingLevel::H5
                    | pulldown_cmark::HeadingLevel::H6 => "#### ".to_string(),
                };
                let heading_style = match level {
                    pulldown_cmark::HeadingLevel::H1 => Style::default()
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::UNDERLINED),
                    pulldown_cmark::HeadingLevel::H2 => {
                        Style::default().add_modifier(Modifier::BOLD)
                    }
                    pulldown_cmark::HeadingLevel::H3 => Style::default()
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::ITALIC),
                    pulldown_cmark::HeadingLevel::H4
                    | pulldown_cmark::HeadingLevel::H5
                    | pulldown_cmark::HeadingLevel::H6 => {
                        Style::default().add_modifier(Modifier::ITALIC)
                    }
                };
                style_stack.push(heading_style);
            }
            Event::End(TagEnd::Heading(_)) => {
                let heading_style = *style_stack.last().unwrap_or(&Style::default());
                push_wrapped_spans_with_prefix(
                    &mut current,
                    &mut out,
                    width,
                    Line::from(vec![Span::styled(heading_prefix.clone(), heading_style)]),
                    Line::from(" ".repeat(display_width(&heading_prefix))),
                );
                current.clear();
                out.push(Line::raw(""));
                heading_prefix.clear();
                in_heading = false;
                style_stack.pop();
            }
            Event::Start(Tag::List(start)) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                list_stack.push(start);
            }
            Event::Start(Tag::Table(_)) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                table_rows.clear();
                table_row.clear();
            }
            Event::End(TagEnd::Table) => {
                flush(&mut current, &mut out, width, "");
                if !table_row.is_empty() {
                    table_rows.push(std::mem::take(&mut table_row));
                }
                render_table(&table_rows, width, &mut out);
                out.push(Line::raw(""));
                table_rows.clear();
            }
            Event::Start(Tag::TableHead) => {}
            Event::End(TagEnd::TableHead) => {
                flush(&mut current, &mut out, width, "");
                if !table_row.is_empty() {
                    table_rows.push(std::mem::take(&mut table_row));
                }
            }
            Event::Start(Tag::TableRow) => {
                flush(&mut current, &mut out, width, "");
                table_row.clear();
            }
            Event::End(TagEnd::TableRow) => {
                flush(&mut current, &mut out, width, "");
                if !table_row.is_empty() {
                    table_rows.push(std::mem::take(&mut table_row));
                }
            }
            Event::Start(Tag::TableCell) => {
                flush(&mut current, &mut out, width, "");
            }
            Event::End(TagEnd::TableCell) => {
                let cell_text = current
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .trim()
                    .to_string();
                table_row.push(cell_text);
                current.clear();
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                quote_depth = quote_depth.saturating_add(1);
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .fg(Color::Green),
                );
            }
            Event::End(TagEnd::BlockQuote) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                quote_depth = quote_depth.saturating_sub(1);
                style_stack.pop();
                out.push(Line::raw(""));
            }
            Event::End(TagEnd::List(_)) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                list_stack.pop();
                line_prefix.clear();
                out.push(Line::raw(""));
            }
            Event::Start(Tag::Item) => {
                let prefix = current_prefix(quote_depth, &line_prefix);
                flush(&mut current, &mut out, width, &prefix);
                let indent = "    ".repeat(list_stack.len().saturating_sub(1));
                line_prefix = match list_stack.last_mut() {
                    Some(Some(number)) => {
                        let prefix = format!("{indent}{number}. ");
                        *number += 1;
                        prefix
                    }
                    Some(None) => format!("{indent}- "),
                    None => "- ".to_string(),
                };
            }
            Event::End(TagEnd::Item) => {
                let prefix = if in_heading {
                    current_prefix(quote_depth, &heading_prefix)
                } else {
                    current_prefix(quote_depth, &line_prefix)
                };
                flush(&mut current, &mut out, width, &prefix);
                line_prefix.clear();
            }
            Event::Start(Tag::Emphasis) => {
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .add_modifier(Modifier::ITALIC),
                );
            }
            Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            Event::Text(text) => {
                if in_code_block {
                    code_buf.push_str(&text);
                } else {
                    current.push(Span::styled(text.to_string(), *style_stack.last().unwrap()));
                }
            }
            Event::Code(text) => {
                current.push(Span::styled(
                    text.to_string(),
                    Style::default().fg(Color::Cyan),
                ));
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_stack.push(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                if let Some(dest) = link_stack.pop()
                    && !dest.is_empty()
                {
                    current.push(Span::raw(" ("));
                    current.push(Span::styled(
                        dest,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                    current.push(Span::raw(")"));
                }
            }
            Event::Start(Tag::Strikethrough) => {
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .add_modifier(Modifier::CROSSED_OUT),
                );
            }
            Event::End(TagEnd::Strikethrough) => {
                style_stack.pop();
            }
            Event::Start(Tag::Strong) => {
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap()
                        .add_modifier(Modifier::BOLD),
                );
            }
            Event::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            Event::Start(Tag::Paragraph) => {
                let prefix = if in_heading {
                    current_prefix(quote_depth, &heading_prefix)
                } else {
                    current_prefix(quote_depth, &line_prefix)
                };
                flush(&mut current, &mut out, width, &prefix);
            }
            Event::End(TagEnd::Paragraph) => {
                let prefix = if in_heading {
                    current_prefix(quote_depth, &heading_prefix)
                } else {
                    current_prefix(quote_depth, &line_prefix)
                };
                flush(&mut current, &mut out, width, &prefix);
                out.push(Line::raw(""));
            }
            Event::SoftBreak | Event::HardBreak => {
                let prefix = if in_heading {
                    current_prefix(quote_depth, &heading_prefix)
                } else {
                    current_prefix(quote_depth, &line_prefix)
                };
                flush(&mut current, &mut out, width, &prefix);
            }
            Event::Rule => out.push(Line::from("———")),
            Event::Html(text) => current.push(Span::styled(
                text.to_string(),
                Style::default().fg(Color::Rgb(140, 150, 170)),
            )),
            Event::FootnoteReference(text) => current.push(Span::raw(text.to_string())),
            Event::TaskListMarker(done) => {
                current.push(Span::raw(if done { "[x] " } else { "[ ] " }));
            }
            _ => {}
        }
    }

    if !current.is_empty() {
        push_wrapped_spans(&mut current, &mut out, width, "");
    }
    out = repair_source_list_indents(out, &input);
    while out.last().is_some_and(|line| line.spans.is_empty()) {
        out.pop();
    }
    out
}

fn current_prefix(quote_depth: usize, line_prefix: &str) -> String {
    let mut prefix = "> ".repeat(quote_depth);
    prefix.push_str(line_prefix);
    prefix
}

fn repair_source_list_indents(lines: Vec<Line<'static>>, source: &str) -> Vec<Line<'static>> {
    let mut pending = source
        .lines()
        .filter_map(source_list_indent_repair)
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return lines;
    }

    lines
        .into_iter()
        .map(|line| {
            let rendered = line_text(&line);
            let Some(index) = pending
                .iter()
                .position(|repair| repair.rendered == rendered)
            else {
                return line;
            };
            let repair = pending.remove(index);
            prepend_raw_prefix(line, repair.prefix)
        })
        .collect()
}

struct ListIndentRepair {
    rendered: String,
    prefix: String,
}

fn source_list_indent_repair(line: &str) -> Option<ListIndentRepair> {
    let leading_spaces = line.chars().take_while(|ch| *ch == ' ').count();
    if leading_spaces == 0 {
        return None;
    }

    let trimmed = &line[leading_spaces..];
    if !looks_like_list_marker(trimmed) {
        return None;
    }

    Some(ListIndentRepair {
        rendered: trimmed.to_string(),
        prefix: " ".repeat(leading_spaces),
    })
}

fn looks_like_list_marker(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }

    let Some((number, rest)) = trimmed.split_once(". ") else {
        return false;
    };
    !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()) && !rest.is_empty()
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

fn prepend_raw_prefix(mut line: Line<'static>, prefix: String) -> Line<'static> {
    let mut spans = Vec::with_capacity(line.spans.len() + 1);
    spans.push(Span::raw(prefix));
    spans.extend(line.spans);
    line.spans = spans;
    line
}

pub(super) fn render_plaintext(input: &str, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for paragraph in normalize_markdown_indentation(input).split('\n') {
        if paragraph.trim().is_empty() {
            out.push(Line::raw(""));
            continue;
        }

        out.extend(
            word_wrap_text(
                paragraph,
                WrapOptions::new(width).initial_indent(Line::default()),
            )
            .into_iter()
            .map(|line| {
                let spans = line
                    .spans
                    .into_iter()
                    .map(|span| {
                        Span::styled(
                            span.content.into_owned(),
                            Style::default().fg(Color::Rgb(200, 200, 210)),
                        )
                    })
                    .collect::<Vec<_>>();
                Line::from(spans)
            }),
        );
    }

    while out.last().is_some_and(|line| line.spans.is_empty()) {
        out.pop();
    }
    out
}

fn render_table(rows: &[Vec<String>], width: usize, out: &mut Vec<Line<'static>>) {
    if rows.is_empty() {
        return;
    }

    let column_count = rows.iter().map(|row| row.len()).max().unwrap_or(0);
    if column_count == 0 {
        return;
    }

    let mut widths = vec![3usize; column_count];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(cell));
        }
    }

    fit_table_widths(&mut widths, width, column_count);

    for (row_index, row) in rows.iter().enumerate() {
        let wrapped_cells = (0..column_count)
            .map(|column| {
                wrap_table_cell(
                    row.get(column).map(String::as_str).unwrap_or(""),
                    widths[column],
                )
            })
            .collect::<Vec<_>>();
        let row_height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);

        for line_index in 0..row_height {
            let mut rendered = String::new();
            for column in 0..column_count {
                if column > 0 {
                    rendered.push_str(" | ");
                }
                let cell_line = wrapped_cells[column]
                    .get(line_index)
                    .map(String::as_str)
                    .unwrap_or("");
                rendered.push_str(cell_line);
                let padding = widths[column].saturating_sub(display_width(cell_line));
                if padding > 0 {
                    rendered.push_str(&" ".repeat(padding));
                }
            }
            out.push(Line::from(vec![Span::styled(
                rendered,
                Style::default().fg(Color::Rgb(200, 200, 210)),
            )]));
        }

        if row_index == 0 && rows.len() > 1 {
            out.push(Line::from(vec![Span::styled(
                table_separator(&widths),
                Style::default().fg(Color::Rgb(90, 96, 108)),
            )]));
        }
    }
}

fn fit_table_widths(widths: &mut [usize], width: usize, column_count: usize) {
    let separator_width = column_count.saturating_sub(1) * 3;
    let mut total_width = widths.iter().sum::<usize>() + separator_width;
    let min_column_width = 6usize;
    let max_content_width = width.max(column_count * min_column_width + separator_width);

    while total_width > max_content_width {
        if let Some((index, _)) = widths.iter().enumerate().max_by_key(|(_, value)| **value) {
            if widths[index] <= min_column_width {
                break;
            }
            widths[index] = widths[index].saturating_sub(1);
            total_width = widths.iter().sum::<usize>() + separator_width;
        } else {
            break;
        }
    }
}

fn wrap_table_cell(cell: &str, width: usize) -> Vec<String> {
    if cell.trim().is_empty() {
        return vec![String::new()];
    }

    textwrap::wrap(cell, width.max(4))
        .into_iter()
        .map(|line| line.into_owned())
        .collect()
}

fn table_separator(widths: &[usize]) -> String {
    widths
        .iter()
        .enumerate()
        .map(|(index, col_width)| {
            let dash = "─".repeat(*col_width);
            if index == 0 {
                dash
            } else {
                format!("─┼─{dash}")
            }
        })
        .collect::<String>()
}

fn normalize_markdown_indentation(input: &str) -> String {
    let min_indent = input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|ch| *ch == ' ').count())
        .min()
        .unwrap_or(0);

    if min_indent == 0 {
        return input.to_string();
    }

    input
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line.chars().skip(min_indent).collect()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_wrapped_spans(
    spans: &mut [Span<'static>],
    out: &mut Vec<Line<'static>>,
    width: usize,
    prefix: &str,
) {
    push_wrapped_spans_with_prefix(
        spans,
        out,
        width,
        Line::from(prefix.to_string()),
        Line::from(" ".repeat(display_width(prefix))),
    );
}

fn push_wrapped_spans_with_prefix(
    spans: &mut [Span<'static>],
    out: &mut Vec<Line<'static>>,
    width: usize,
    initial_prefix: Line<'static>,
    subsequent_prefix: Line<'static>,
) {
    let wrapped = word_wrap_spans(
        spans,
        WrapOptions::new(width)
            .initial_indent(initial_prefix)
            .subsequent_indent(subsequent_prefix),
    );
    out.extend(wrapped);
}

fn highlight_code_line(line: &str, _lang: &str) -> Vec<Span<'static>> {
    vec![Span::styled(
        line.to_string(),
        Style::default().fg(Color::Rgb(210, 210, 220)),
    )]
}

#[allow(dead_code)]
fn grapheme_len(value: &str) -> usize {
    UnicodeSegmentation::graphemes(value, true).count()
}

#[cfg(test)]
mod tests {
    use super::{render_markdown, render_plaintext};
    use ratatui::style::Color;
    use ratatui::style::Modifier;

    fn joined(lines: &[ratatui::text::Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn tables_keep_column_separators() {
        let input = "\
| 风险 | 根因 | 优先级 |\n\
| --- | --- | --- |\n\
| final budget exceeded | 只 log 不 block | 高 |\n\
| token estimator | len vs chars | 中 |";

        let rendered = render_markdown(input, 120);
        let text = joined(&rendered);

        assert!(text.contains("风险"));
        assert!(text.contains(" | "));
        assert!(text.contains("只 log 不 block"));
    }

    #[test]
    fn plaintext_preserves_line_breaks() {
        let rendered = render_plaintext("first line\nsecond line", 40);
        let text = joined(&rendered);

        assert_eq!(text, "first line\nsecond line");
    }

    #[test]
    fn tables_wrap_long_cells_inside_columns() {
        let input = "\
| section | detail |\n\
| --- | --- |\n\
| command | this is a very long detail cell that should wrap inside the table column |";

        let rendered = render_markdown(input, 44);
        let text = joined(&rendered);

        assert!(text.contains("section"));
        assert!(text.contains("detail"));
        assert!(text.contains("command"));
        assert!(text.contains(" | "));
        assert!(text.lines().count() > 3);
    }

    #[test]
    fn heading_markers_use_heading_style() {
        let rendered = render_markdown("## Summary\n\nBody", 80);
        let heading = rendered.first().expect("heading line");

        assert_eq!(heading.spans[0].content.as_ref(), "## ");
        assert!(heading.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(heading.spans[1].content.as_ref(), "Summary");
        assert!(heading.spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn fenced_code_blocks_do_not_fill_the_line_background() {
        let rendered = render_markdown("```text\nlet value = 1;\n```", 80);
        let code = rendered.first().expect("code line");
        let text = code
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(text, "let value = 1;");
        assert_eq!(code.style.bg, None);
        assert!(
            code.spans
                .iter()
                .all(|span| span.style.bg != Some(Color::Rgb(25, 28, 35)))
        );
    }

    #[test]
    fn fenced_code_blocks_without_language_are_not_indented_as_indented_code() {
        let rendered = render_markdown("```\nplain\n```", 80);
        let code = rendered.first().expect("code line");
        let text = code
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(text, "plain");
    }

    #[test]
    fn renders_codex_style_common_markdown_blocks() {
        let input = concat!(
            "Intro with `code`, [docs](https://example.com), and escaped \\*literal\\*.\n",
            "\n",
            "> quoted\n",
            "> - item\n",
            "\n",
            "- outer\n",
            "    - inner\n",
            "- [x] done\n",
            "- [ ] todo\n",
            "\n",
            "---\n",
            "\n",
            "1. one\n",
            "2. two",
        );
        let rendered = render_markdown(input, 80);
        let text = joined(&rendered);

        assert!(
            text.contains("Intro with code, docs (https://example.com), and escaped *literal*.")
        );
        assert!(text.contains("> quoted"));
        assert!(text.contains("> - item"));
        assert!(text.contains("- outer"));
        assert!(text.contains("    - inner"), "{text}");
        assert!(text.contains("- [x] done"));
        assert!(text.contains("- [ ] todo"));
        assert!(text.contains("———"));
        assert!(text.contains("1. one"));
        assert!(text.contains("2. two"));
    }

    #[test]
    fn inline_code_uses_text_only_without_background_padding() {
        let rendered = render_markdown("Use `cargo test` now.", 80);
        let line = rendered.first().expect("line");
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(text, "Use cargo test now.");
        assert!(
            line.spans
                .iter()
                .any(|span| span.style.fg == Some(Color::Cyan)),
            "{line:?}"
        );
        assert!(line.spans.iter().all(|span| span.style.bg.is_none()));
    }

    #[test]
    fn source_list_indent_repair_preserves_nested_marker_columns() {
        let repaired = super::repair_source_list_indents(
            vec![ratatui::text::Line::from("- inner")],
            "    - inner",
        );

        assert_eq!(joined(&repaired), "    - inner");
    }

    #[test]
    fn nested_list_source_indent_survives_render_markdown() {
        let rendered = render_markdown("- outer\n    - inner", 80);
        assert_eq!(joined(&rendered), "- outer\n    - inner");
    }

    #[test]
    fn nested_list_source_indent_repair_works_with_surrounding_markdown() {
        let source = "Intro\n\n- outer\n    - inner\n\n---";
        let repaired =
            super::repair_source_list_indents(vec![ratatui::text::Line::from("- inner")], source);
        assert_eq!(joined(&repaired), "    - inner");
    }
}
