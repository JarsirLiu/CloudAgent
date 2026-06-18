use super::wrapping::{WrapOptions, word_wrap_spans, word_wrap_text};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use crate::text_width::display_width;

pub(super) fn render_markdown(input: &str, width: usize) -> Vec<Line<'static>> {
    let input = normalize_markdown_indentation(input);
    MarkdownWriter::new(&input, width).render()
}

struct MarkdownWriter<'a> {
    input: &'a str,
    width: usize,
    out: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    table_cell: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    in_code_block: bool,
    code_indent: String,
    code_buf: String,
    indent_stack: Vec<IndentContext>,
    list_stack: Vec<Option<u64>>,
    list_needs_blank_before_next_item: Vec<bool>,
    list_item_start_line_counts: Vec<usize>,
    heading_prefix: String,
    table_rows: Vec<Vec<String>>,
    table_row: Vec<String>,
    in_table_cell: bool,
    link_stack: Vec<String>,
    needs_newline: bool,
    pending_marker_line: bool,
}

#[derive(Clone, Debug)]
struct IndentContext {
    prefix: Vec<Span<'static>>,
    marker: Option<Vec<Span<'static>>>,
    is_list: bool,
}

impl IndentContext {
    fn new(prefix: Vec<Span<'static>>, marker: Option<Vec<Span<'static>>>, is_list: bool) -> Self {
        Self {
            prefix,
            marker,
            is_list,
        }
    }
}

impl<'a> MarkdownWriter<'a> {
    fn new(input: &'a str, width: usize) -> Self {
        Self {
            input,
            width,
            out: Vec::new(),
            current: Vec::new(),
            table_cell: Vec::new(),
            style_stack: vec![Style::default().fg(Color::Rgb(200, 200, 210))],
            in_code_block: false,
            code_indent: String::new(),
            code_buf: String::new(),
            indent_stack: Vec::new(),
            list_stack: Vec::new(),
            list_needs_blank_before_next_item: Vec::new(),
            list_item_start_line_counts: Vec::new(),
            heading_prefix: String::new(),
            table_rows: Vec::new(),
            table_row: Vec::new(),
            in_table_cell: false,
            link_stack: Vec::new(),
            needs_newline: false,
            pending_marker_line: false,
        }
    }

    fn render(mut self) -> Vec<Line<'static>> {
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_TASKLISTS);

        for event in Parser::new_ext(self.input, opts) {
            self.handle_event(event);
        }

        self.flush_current();
        self.out = repair_source_list_indents(self.out, self.input);
        while self.out.last().is_some_and(|line| line.spans.is_empty()) {
            self.out.pop();
        }
        self.out
    }

    fn handle_event(&mut self, event: Event<'a>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(text.as_ref()),
            Event::Code(text) => self.code(text.as_ref()),
            Event::SoftBreak | Event::HardBreak => self.line_break(),
            Event::Rule => self.rule(),
            Event::Html(text) => self.html(text.as_ref()),
            Event::FootnoteReference(text) => self.push_text_span(text.as_ref()),
            Event::TaskListMarker(done) => {
                self.push_span(Span::raw(if done { "[x] " } else { "[ ] " }));
            }
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => self.start_paragraph(),
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::BlockQuote(_) => self.start_blockquote(),
            Tag::CodeBlock(kind) => self.start_code_block(kind),
            Tag::List(start) => self.start_list(start),
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.push_style(Modifier::ITALIC),
            Tag::Strong => self.push_style(Modifier::BOLD),
            Tag::Strikethrough => self.push_style(Modifier::CROSSED_OUT),
            Tag::Link { dest_url, .. } => self.link_stack.push(dest_url.to_string()),
            Tag::Table(_) => self.start_table(),
            Tag::TableHead => {}
            Tag::TableRow => {
                self.table_row.clear();
            }
            Tag::TableCell => {
                self.in_table_cell = true;
                self.table_cell.clear();
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::BlockQuote => self.end_blockquote(),
            TagEnd::CodeBlock => self.end_code_block(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => self.end_item(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            TagEnd::Link => self.end_link(),
            TagEnd::Table => self.end_table(),
            TagEnd::TableHead if !self.table_row.is_empty() => {
                self.table_rows.push(std::mem::take(&mut self.table_row));
            }
            TagEnd::TableRow if !self.table_row.is_empty() => {
                self.table_rows.push(std::mem::take(&mut self.table_row));
            }
            TagEnd::TableCell => {
                let cell_text = self
                    .table_cell
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .trim()
                    .to_string();
                self.table_row.push(cell_text);
                self.table_cell.clear();
                self.in_table_cell = false;
            }
            _ => {}
        }
    }

    fn start_paragraph(&mut self) {
        if self.needs_newline {
            self.push_blank_line();
        }
        self.needs_newline = false;
    }

    fn end_paragraph(&mut self) {
        self.flush_current();
        self.needs_newline = true;
    }

    fn start_heading(&mut self, level: pulldown_cmark::HeadingLevel) {
        self.flush_current();
        if self.needs_newline {
            self.push_blank_line();
        }
        self.heading_prefix = match level {
            pulldown_cmark::HeadingLevel::H1 => "# ".to_string(),
            pulldown_cmark::HeadingLevel::H2 => "## ".to_string(),
            pulldown_cmark::HeadingLevel::H3 => "### ".to_string(),
            pulldown_cmark::HeadingLevel::H4 => "#### ".to_string(),
            pulldown_cmark::HeadingLevel::H5 => "##### ".to_string(),
            pulldown_cmark::HeadingLevel::H6 => "###### ".to_string(),
        };
        let heading_style = match level {
            pulldown_cmark::HeadingLevel::H1 => Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
            pulldown_cmark::HeadingLevel::H2 => Style::default().add_modifier(Modifier::BOLD),
            pulldown_cmark::HeadingLevel::H3 => Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::ITALIC),
            pulldown_cmark::HeadingLevel::H4
            | pulldown_cmark::HeadingLevel::H5
            | pulldown_cmark::HeadingLevel::H6 => Style::default().add_modifier(Modifier::ITALIC),
        };
        self.style_stack.push(heading_style);
        self.needs_newline = false;
    }

    fn end_heading(&mut self) {
        let heading_style = *self.style_stack.last().unwrap_or(&Style::default());
        self.current
            .insert(0, Span::styled(self.heading_prefix.clone(), heading_style));
        self.flush_current();
        self.current.clear();
        self.heading_prefix.clear();
        self.style_stack.pop();
        self.needs_newline = true;
    }

    fn start_blockquote(&mut self) {
        self.flush_current();
        if self.needs_newline {
            self.push_blank_line();
        }
        self.indent_stack
            .push(IndentContext::new(vec![Span::from("> ")], None, false));
        self.style_stack.push(
            self.style_stack
                .last()
                .copied()
                .unwrap_or_default()
                .fg(Color::Green),
        );
        self.needs_newline = false;
    }

    fn end_blockquote(&mut self) {
        self.flush_current();
        self.indent_stack.pop();
        self.style_stack.pop();
        self.needs_newline = true;
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'a>) {
        self.flush_current();
        if !self.out.is_empty() {
            self.push_blank_line();
        }
        self.in_code_block = true;
        self.code_indent = match kind {
            CodeBlockKind::Fenced(_) => String::new(),
            CodeBlockKind::Indented => "    ".to_string(),
        };
        self.code_buf.clear();
        self.needs_newline = false;
    }

    fn end_code_block(&mut self) {
        self.in_code_block = false;
        let prefix = format!("{}{}", self.current_prefix_line(), self.code_indent);
        for line in self.code_buf.lines() {
            push_code_line(line, &prefix, &mut self.out);
        }
        self.code_indent.clear();
        self.needs_newline = true;
    }

    fn start_list(&mut self, start: Option<u64>) {
        self.flush_current();
        if self.list_stack.is_empty() && self.needs_newline {
            self.push_blank_line();
        }
        self.list_stack.push(start);
        self.list_needs_blank_before_next_item.push(false);
        self.needs_newline = false;
    }

    fn end_list(&mut self) {
        self.flush_current();
        self.list_stack.pop();
        self.list_needs_blank_before_next_item.pop();
        self.needs_newline = true;
    }

    fn start_item(&mut self) {
        if self
            .list_needs_blank_before_next_item
            .last_mut()
            .map(std::mem::take)
            .unwrap_or(false)
        {
            self.push_blank_line();
        }
        self.flush_current();
        self.list_item_start_line_counts.push(self.out.len());
        let depth = self.list_stack.len();
        let is_ordered = self.list_stack.last().map(Option::is_some).unwrap_or(false);
        let width = depth * 4 - 3;
        let marker = match self.list_stack.last_mut() {
            Some(Some(number)) => {
                let prefix = format!("{number}. ");
                *number += 1;
                Some(vec![Span::styled(
                    prefix,
                    Style::default().fg(Color::Rgb(150, 180, 255)),
                )])
            }
            Some(None) => Some(vec![Span::styled(
                "- ".to_string(),
                Style::default().fg(Color::Rgb(200, 200, 210)),
            )]),
            None => Some(vec![Span::raw("- ".to_string())]),
        };
        let prefix = if depth == 0 {
            Vec::new()
        } else {
            let indent_len = if is_ordered { width + 2 } else { width + 1 };
            vec![Span::from(" ".repeat(indent_len))]
        };
        self.indent_stack
            .push(IndentContext::new(prefix, marker, true));
        self.pending_marker_line = true;
        self.needs_newline = false;
    }

    fn end_item(&mut self) {
        self.flush_current();
        let start_line_count = self.list_item_start_line_counts.pop().unwrap_or_default();
        if self.out.len().saturating_sub(start_line_count) > 1
            && let Some(needs_blank) = self.list_needs_blank_before_next_item.last_mut()
        {
            *needs_blank = true;
        }
        self.indent_stack.pop();
        self.pending_marker_line = false;
    }

    fn start_table(&mut self) {
        self.flush_current();
        if self.needs_newline {
            self.push_blank_line();
        }
        self.table_rows.clear();
        self.table_row.clear();
        self.needs_newline = false;
    }

    fn end_table(&mut self) {
        if !self.table_row.is_empty() {
            self.table_rows.push(std::mem::take(&mut self.table_row));
        }
        render_table(&self.table_rows, self.width, &mut self.out);
        self.table_rows.clear();
        self.needs_newline = true;
    }

    fn text(&mut self, text: &str) {
        if self.in_code_block {
            self.code_buf.push_str(text);
            return;
        }
        if self.in_table_cell {
            self.table_cell.push(Span::styled(
                text.to_string(),
                *self.style_stack.last().unwrap(),
            ));
            return;
        }
        self.push_text_span(text);
    }

    fn code(&mut self, text: &str) {
        let span = Span::styled(text.to_string(), Style::default().fg(Color::Cyan));
        if self.in_table_cell {
            self.table_cell.push(span);
        } else {
            self.push_span(span);
        }
    }

    fn html(&mut self, text: &str) {
        if self.in_table_cell {
            self.table_cell.push(Span::styled(
                text.to_string(),
                Style::default().fg(Color::Rgb(140, 150, 170)),
            ));
        } else {
            self.push_span(Span::styled(
                text.to_string(),
                Style::default().fg(Color::Rgb(140, 150, 170)),
            ));
        }
    }

    fn end_link(&mut self) {
        if let Some(dest) = self.link_stack.pop()
            && !dest.is_empty()
        {
            self.push_span(Span::raw(" ("));
            self.push_span(Span::styled(
                dest,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            self.push_span(Span::raw(")"));
        }
    }

    fn line_break(&mut self) {
        if self.in_table_cell {
            self.table_cell.push(Span::raw(" "));
            return;
        }
        self.flush_current();
    }

    fn rule(&mut self) {
        self.flush_current();
        if !self.out.is_empty() {
            self.push_blank_line();
        }
        self.out.push(Line::from("———"));
        self.needs_newline = true;
    }

    fn push_text_span(&mut self, text: &str) {
        self.current.push(Span::styled(
            text.to_string(),
            *self.style_stack.last().unwrap(),
        ));
    }

    fn push_span(&mut self, span: Span<'static>) {
        self.current.push(span);
    }

    fn push_style(&mut self, modifier: Modifier) {
        self.style_stack.push(
            self.style_stack
                .last()
                .copied()
                .unwrap_or_default()
                .add_modifier(modifier),
        );
    }

    fn flush_current(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let prefix = self.current_prefix_line();
        push_wrapped_spans(&mut self.current, &mut self.out, self.width, &prefix);
        self.current.clear();
        self.pending_marker_line = false;
    }

    fn push_blank_line(&mut self) {
        self.flush_current();
        if self.out.last().is_some_and(|line| line.spans.is_empty()) {
            return;
        }

        let prefix = self.current_prefix_line();
        if prefix.is_empty() {
            self.out.push(Line::raw(""));
        } else {
            self.out.push(Line::from(prefix));
        }
    }

    fn current_prefix_line(&self) -> String {
        let mut prefix = String::new();
        let marker_index = if self.pending_marker_line {
            self.indent_stack
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, ctx)| ctx.marker.as_ref().map(|_| index))
        } else {
            None
        };
        let last_list_index = self.indent_stack.iter().rposition(|ctx| ctx.is_list);

        for (index, ctx) in self.indent_stack.iter().enumerate() {
            if self.pending_marker_line {
                if Some(index) == marker_index
                    && let Some(marker) = &ctx.marker
                {
                    prefix.extend(marker.iter().map(|span| span.content.as_ref()));
                    continue;
                }
                if ctx.is_list && marker_index.is_some_and(|marker| marker > index) {
                    continue;
                }
            } else if ctx.is_list && Some(index) != last_list_index {
                continue;
            }

            prefix.extend(ctx.prefix.iter().map(|span| span.content.as_ref()));
        }

        prefix
    }
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

fn push_code_line(line: &str, prefix: &str, out: &mut Vec<Line<'static>>) {
    let mut spans = Vec::new();
    if !prefix.is_empty() {
        spans.push(Span::raw(prefix.to_string()));
    }
    spans.push(Span::styled(
        line.to_string(),
        Style::default().fg(Color::Rgb(210, 210, 220)),
    ));
    out.push(Line::from(spans));
}

#[allow(dead_code)]
fn grapheme_len(value: &str) -> usize {
    UnicodeSegmentation::graphemes(value, true).count()
}

#[cfg(test)]
mod tests;
