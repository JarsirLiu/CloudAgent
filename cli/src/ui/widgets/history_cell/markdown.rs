use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) fn render_markdown(input: &str, width: usize) -> Vec<Line<'static>> {
    let input = normalize_markdown_indentation(input);
    let input = input.replace('\\', "\\\\");
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
    let mut code_buf = String::new();
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut line_prefix = String::new();
    let mut heading_prefix = String::new();
    let mut in_heading = false;

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
                flush(&mut current, &mut out, width, &line_prefix);
                in_code_block = true;
                code_lang = match &kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    _ => String::new(),
                };
                code_buf.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                for line in code_buf.lines() {
                    let mut spans = vec![Span::raw("  ")];
                    spans.extend(highlight_code_line(line, &code_lang));
                    let vis = display_width(
                        &spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>(),
                    );
                    if vis < width {
                        spans.push(Span::raw(" ".repeat(width - vis)));
                    }
                    out.push(Line::from(spans).style(Style::default().bg(Color::Rgb(25, 28, 35))));
                }
                out.push(Line::raw(""));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut current, &mut out, width, &line_prefix);
                in_heading = true;
                heading_prefix = match level {
                    pulldown_cmark::HeadingLevel::H1 => "# ".to_string(),
                    pulldown_cmark::HeadingLevel::H2 => "## ".to_string(),
                    pulldown_cmark::HeadingLevel::H3 => "### ".to_string(),
                    _ => "• ".to_string(),
                };
                style_stack.push(
                    Style::default()
                        .fg(Color::Rgb(170, 190, 255))
                        .add_modifier(Modifier::BOLD),
                );
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut current, &mut out, width, &heading_prefix);
                current.clear();
                out.push(Line::raw(""));
                heading_prefix.clear();
                in_heading = false;
                style_stack.pop();
            }
            Event::Start(Tag::List(start)) => {
                flush(&mut current, &mut out, width, &line_prefix);
                list_stack.push(start);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut current, &mut out, width, &line_prefix);
                line_prefix = "│ ".to_string();
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .fg(Color::Rgb(150, 160, 190)),
                );
            }
            Event::End(TagEnd::BlockQuote) => {
                flush(&mut current, &mut out, width, &line_prefix);
                line_prefix.clear();
                style_stack.pop();
                out.push(Line::raw(""));
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut current, &mut out, width, &line_prefix);
                list_stack.pop();
                line_prefix.clear();
                out.push(Line::raw(""));
            }
            Event::Start(Tag::Item) => {
                flush(&mut current, &mut out, width, &line_prefix);
                let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                line_prefix = match list_stack.last_mut() {
                    Some(Some(number)) => {
                        let prefix = format!("{indent}{number}. ");
                        *number += 1;
                        prefix
                    }
                    Some(None) => format!("{indent}• "),
                    None => "• ".to_string(),
                };
            }
            Event::End(TagEnd::Item) => {
                let prefix = if in_heading {
                    heading_prefix.as_str()
                } else {
                    line_prefix.as_str()
                };
                flush(&mut current, &mut out, width, prefix);
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
                    format!(" {text} "),
                    Style::default()
                        .fg(Color::Rgb(140, 220, 255))
                        .bg(Color::Rgb(30, 35, 45)),
                ));
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
                    heading_prefix.as_str()
                } else {
                    line_prefix.as_str()
                };
                flush(&mut current, &mut out, width, prefix);
            }
            Event::End(TagEnd::Paragraph) => {
                let prefix = if in_heading {
                    heading_prefix.as_str()
                } else {
                    line_prefix.as_str()
                };
                flush(&mut current, &mut out, width, prefix);
                out.push(Line::raw(""));
            }
            Event::SoftBreak | Event::HardBreak => {
                let prefix = if in_heading {
                    heading_prefix.as_str()
                } else {
                    line_prefix.as_str()
                };
                flush(&mut current, &mut out, width, prefix);
            }
            Event::Rule => out.push(Line::from("─".repeat(width.max(3)))),
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
    while out.last().is_some_and(|line| line.spans.is_empty()) {
        out.pop();
    }
    out
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
    let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
    let available = width.saturating_sub(display_width(prefix)).max(8);
    let wrapped = textwrap::wrap(&text, available);
    for (index, wrapped_line) in wrapped.into_iter().enumerate() {
        let mut line_spans = Vec::new();
        if index == 0 && !prefix.is_empty() {
            line_spans.push(Span::raw(prefix.to_string()));
        } else if !prefix.is_empty() {
            line_spans.push(Span::raw(" ".repeat(display_width(prefix))));
        }
        let style = spans.first().map(|span| span.style).unwrap_or_default();
        line_spans.push(Span::styled(wrapped_line.into_owned(), style));
        out.push(Line::from(line_spans));
    }
}

fn highlight_code_line(line: &str, _lang: &str) -> Vec<Span<'static>> {
    vec![Span::styled(
        line.to_string(),
        Style::default().fg(Color::Rgb(210, 210, 220)),
    )]
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

#[allow(dead_code)]
fn grapheme_len(value: &str) -> usize {
    UnicodeSegmentation::graphemes(value, true).count()
}
