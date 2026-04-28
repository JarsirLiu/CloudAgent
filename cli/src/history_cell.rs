use agent_protocol::HistoryEntry;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use textwrap::{Options as WrapOptions, WordSplitter, wrap};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

// ── Shimmer animation ─────────────────────────────────────────────────────────

static SHIMMER_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn elapsed() -> Duration {
    SHIMMER_START.get_or_init(Instant::now).elapsed()
}

pub fn shimmer_spans(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![];
    }
    let padding = 8usize;
    let period = chars.len() + padding * 2;
    let sweep = 2.0f32;
    let pos = ((elapsed().as_secs_f32() % sweep) / sweep * period as f32) as usize;

    let base = Color::Rgb(100, 100, 110);
    let bright = Color::Rgb(200, 200, 220);

    chars
        .into_iter()
        .enumerate()
        .map(|(i, ch)| {
            let dist = ((i + padding) as isize - pos as isize).unsigned_abs();
            let t = if dist < 6 {
                let x = std::f32::consts::PI * (dist as f32 / 6.0);
                0.5 * (1.0 + x.cos())
            } else {
                0.0
            };
            let color = blend_color(base, bright, t * 0.85);
            Span::styled(
                ch.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )
        })
        .collect()
}

fn blend_color(a: Color, b: Color, t: f32) -> Color {
    let (ar, ag, ab) = unpack(a);
    let (br, bg, bb) = unpack(b);
    let r = (ar as f32 + (br as f32 - ar as f32) * t) as u8;
    let g = (ag as f32 + (bg as f32 - ag as f32) * t) as u8;
    let b2 = (ab as f32 + (bb as f32 - ab as f32) * t) as u8;
    Color::Rgb(r, g, b2)
}

fn unpack(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}

// ── History Cell ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryTone {
    User,
    Agent,
    Tool,
    Warning,
    Error,
    Meta,
}

#[derive(Clone, Debug, Default)]
pub struct TranscriptRenderState {
    pub compact_tools: bool,
    pub expanded_tool_cells: HashSet<usize>,
    pub selected_cell: Option<usize>,
    pub matched_cells: HashSet<usize>,
}

#[derive(Clone, Debug)]
pub struct HistoryCell {
    pub label: String,
    pub body: String,
    pub tone: HistoryTone,
    // Simple cache to avoid re-rendering MD on every frame if width is same
    cache: std::sync::Arc<std::sync::Mutex<Option<(usize, Vec<Line<'static>>)>>>,
}

impl HistoryCell {
    pub fn new(label: impl Into<String>, body: impl Into<String>, tone: HistoryTone) -> Self {
        Self {
            label: label.into(),
            body: body.into(),
            tone,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn from_message(
        label: impl Into<String>,
        body: impl Into<String>,
        tone: HistoryTone,
    ) -> Self {
        Self::new(label, body, tone)
    }

    pub fn is_empty(&self) -> bool {
        self.body.trim().is_empty()
    }

    pub fn to_lines_with_mode(&self, width: usize, compact_tools: bool) -> Vec<Line<'static>> {
        if compact_tools && self.tone == HistoryTone::Tool {
            return self.render_tool_compact(width, Color::Rgb(80, 200, 120), "◆");
        }
        if let Ok(mut cache) = self.cache.lock() {
            if let Some((w, lines)) = &*cache {
                if *w == width && !compact_tools {
                    return lines.clone();
                }
            }
            let lines = self.render_now(width);
            if !compact_tools {
                *cache = Some((width, lines.clone()));
            }
            lines
        } else {
            self.render_now(width)
        }
    }

    fn render_now(&self, width: usize) -> Vec<Line<'static>> {
        match self.tone {
            HistoryTone::User => self.render_user(width),
            HistoryTone::Agent => self.render_agent(width),
            HistoryTone::Tool => self.render_tool_like(width, Color::Rgb(80, 200, 120), "◆"),
            HistoryTone::Warning => self.render_tool_like(width, Color::Rgb(255, 180, 50), "◆"),
            HistoryTone::Error => self.render_tool_like(width, Color::Rgb(255, 80, 80), "◆"),
            HistoryTone::Meta => self.render_meta(width),
        }
    }

    fn render_user(&self, width: usize) -> Vec<Line<'static>> {
        let content_width = width.saturating_sub(4).max(8);
        let wrapped = wrap_text(&self.body, content_width);
        let mut lines = vec![Line::raw("")];
        for (idx, text) in wrapped.into_iter().enumerate() {
            let prefix = if idx == 0 { "› " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Rgb(140, 150, 170))),
                Span::styled(
                    text,
                    Style::default()
                        .fg(Color::Rgb(220, 220, 235))
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        lines
    }

    fn render_agent(&self, width: usize) -> Vec<Line<'static>> {
        let inner = width.saturating_sub(6).max(8);
        let md_lines = render_markdown(&self.body, inner);
        let mut out = vec![Line::raw("")];
        for (i, line) in md_lines.into_iter().enumerate() {
            let prefix = if i == 0 {
                Span::styled(" ● ", Style::default().fg(Color::Rgb(100, 180, 255)))
            } else {
                Span::raw("   ")
            };
            let mut spans = vec![prefix];
            spans.extend(line.spans);
            out.push(Line::from(spans));
        }
        out
    }

    fn render_tool_like(&self, width: usize, accent: Color, dot: &str) -> Vec<Line<'static>> {
        let mut lines = vec![Line::raw("")];
        let title = pretty_tool_title(&self.label);
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!(" {dot} "),
                Style::default().fg(accent).bg(Color::Rgb(30, 35, 45)),
            ),
            Span::styled(
                format!(" {} ", title),
                Style::default()
                    .fg(Color::Rgb(200, 200, 210))
                    .bg(Color::Rgb(30, 35, 45))
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        let body_width = width.saturating_sub(8).max(8);
        for line in wrap_text(&self.body, body_width) {
            if !line.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(line, Style::default().fg(Color::Rgb(130, 130, 140))),
                ]));
            }
        }
        lines
    }

    fn render_tool_compact(&self, width: usize, accent: Color, dot: &str) -> Vec<Line<'static>> {
        let title = pretty_tool_title(&self.label);
        let summary = summarize_tool_body(&self.body, width.saturating_sub(24).max(12));
        vec![
            Line::raw(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!(" {dot} "),
                    Style::default().fg(accent).bg(Color::Rgb(30, 35, 45)),
                ),
                Span::styled(
                    format!(" {} ", title),
                    Style::default()
                        .fg(Color::Rgb(200, 200, 210))
                        .bg(Color::Rgb(30, 35, 45))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(summary, Style::default().fg(Color::Rgb(130, 130, 145))),
            ]),
        ]
    }

    fn render_meta(&self, width: usize) -> Vec<Line<'static>> {
        let wrapped = wrap_text(&self.body, width.saturating_sub(4).max(8));
        let mut lines = vec![Line::raw("")];
        for (i, line) in wrapped.into_iter().enumerate() {
            let prefix = if i == 0 { "· " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Rgb(80, 80, 90))),
                Span::styled(line, Style::default().fg(Color::Rgb(110, 110, 120))),
            ]));
        }
        lines
    }

    pub fn is_tool_like(&self) -> bool {
        matches!(
            self.tone,
            HistoryTone::Tool | HistoryTone::Warning | HistoryTone::Error
        )
    }
}

// ── Transcript ────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Transcript {
    cells: Vec<HistoryCell>,
}

impl Transcript {
    pub fn replace_with_history(&mut self, messages: &[HistoryEntry]) {
        self.cells.clear();
        for message in messages {
            let cell = render_history_entry(message);
            if !cell.is_empty() {
                self.cells.push(cell);
            }
        }
    }

    pub fn push(&mut self, cell: HistoryCell) -> usize {
        self.cells.push(cell);
        self.cells.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
    pub fn render_lines(
        &self,
        width: usize,
        height: usize,
        scroll: usize,
        render_state: &TranscriptRenderState,
    ) -> Vec<Line<'static>> {
        let mut all_lines = Vec::new();
        for (idx, cell) in self.cells.iter().enumerate() {
            let compact_tools = render_state.compact_tools
                && cell.tone == HistoryTone::Tool
                && !render_state.expanded_tool_cells.contains(&idx);
            let mut lines = cell.to_lines_with_mode(width, compact_tools);
            decorate_cell_lines(
                &mut lines,
                render_state.selected_cell == Some(idx),
                render_state.matched_cells.contains(&idx),
                cell.is_tool_like(),
            );
            all_lines.extend(lines);
        }
        let total = all_lines.len();

        if total == 0 {
            return vec![];
        }

        // Bottom-aligned logic:
        // We want the last 'height' lines when scroll is 0.
        let end = total.saturating_sub(scroll);
        let start = end.saturating_sub(height);

        let mut result: Vec<Line<'static>> = all_lines
            .into_iter()
            .skip(start)
            .take(end - start)
            .collect();

        // If we have less lines than height and we are at the bottom (scroll=0),
        // we can pad with empty lines at the TOP to push content to bottom.
        if result.len() < height && scroll == 0 {
            let mut padded = vec![Line::raw(""); height - result.len()];
            padded.extend(result);
            result = padded;
        }

        result
    }

    pub fn total_lines_with_state(
        &self,
        width: usize,
        render_state: &TranscriptRenderState,
    ) -> usize {
        self.cells
            .iter()
            .enumerate()
            .map(|(idx, cell)| {
                let compact = render_state.compact_tools
                    && cell.tone == HistoryTone::Tool
                    && !render_state.expanded_tool_cells.contains(&idx);
                cell.to_lines_with_mode(width, compact).len()
            })
            .sum()
    }

    pub fn tool_cell_indices(&self) -> Vec<usize> {
        self.cells
            .iter()
            .enumerate()
            .filter_map(|(idx, cell)| (cell.tone == HistoryTone::Tool).then_some(idx))
            .collect()
    }

    pub fn update_cell_body(&mut self, index: usize, body: String) -> bool {
        let Some(cell) = self.cells.get_mut(index) else {
            return false;
        };
        cell.body = body;
        if let Ok(mut cache) = cell.cache.lock() {
            *cache = None;
        }
        true
    }
}

// ── Event Helpers ─────────────────────────────────────────────────────────────

pub fn render_history_entry(message: &HistoryEntry) -> HistoryCell {
    match message {
        HistoryEntry::System { content } => {
            HistoryCell::from_message("system", content.clone(), HistoryTone::Meta)
        }
        HistoryEntry::User { content } => {
            HistoryCell::from_message("you", content.clone(), HistoryTone::User)
        }
        HistoryEntry::Assistant {
            content,
            has_tool_calls,
        } => {
            let body = content.clone().unwrap_or_else(|| {
                if *has_tool_calls {
                    "Working...".into()
                } else {
                    "".into()
                }
            });
            HistoryCell::from_message("cloudagent", body, HistoryTone::Agent)
        }
        HistoryEntry::Tool { name, content, .. } => {
            HistoryCell::from_message(name.clone(), content.clone(), HistoryTone::Tool)
        }
    }
}

// ── Markdown ──────────────────────────────────────────────────────────────────

fn render_markdown(input: &str, width: usize) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(input, opts);

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
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    _ => "".into(),
                };
                code_buf.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                let buf = code_buf.clone();
                for line in buf.lines() {
                    let mut spans = vec![Span::raw("  ")];
                    spans.extend(highlight_code_line(line, &code_lang));
                    let vis = display_width(
                        &spans.iter().map(|s| s.content.as_ref()).collect::<String>(),
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
                let heading_style = Style::default()
                    .fg(Color::Rgb(170, 190, 255))
                    .add_modifier(Modifier::BOLD);
                style_stack.push(heading_style);
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
                let style = style_stack
                    .last()
                    .copied()
                    .unwrap_or_default()
                    .add_modifier(Modifier::ITALIC);
                style_stack.push(style);
            }
            Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            Event::Text(text) => {
                if in_code_block {
                    code_buf.push_str(&text);
                } else {
                    let style = *style_stack.last().unwrap();
                    current.push(Span::styled(text.to_string(), style));
                }
            }
            Event::Code(text) => {
                let inline_style = Style::default()
                    .fg(Color::Rgb(140, 220, 255))
                    .bg(Color::Rgb(30, 35, 45));
                current.push(Span::styled(format!(" {text} "), inline_style));
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
                style_stack.push(style_stack.last().unwrap().add_modifier(Modifier::BOLD));
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
            _ => {}
        }
    }
    let prefix = if in_heading {
        heading_prefix.as_str()
    } else {
        line_prefix.as_str()
    };
    flush(&mut current, &mut out, width, prefix);
    out
}

fn push_wrapped_spans(
    spans: &[Span<'static>],
    out: &mut Vec<Line<'static>>,
    width: usize,
    prefix: &str,
) {
    let prefix_width = display_width(prefix);
    let content_width = width.saturating_sub(prefix_width).max(1);
    let continuation = " ".repeat(prefix_width);
    let mut line_spans: Vec<Span<'static>> = Vec::new();
    let mut line_width = 0usize;
    let mut first_line = true;

    let push_line = |line_spans: &mut Vec<Span<'static>>,
                     out: &mut Vec<Line<'static>>,
                     first_line: &mut bool| {
        let mut full = Vec::new();
        if !prefix.is_empty() {
            let leader = if *first_line {
                prefix
            } else {
                continuation.as_str()
            };
            full.push(Span::styled(
                leader.to_string(),
                Style::default().fg(Color::Rgb(120, 130, 170)),
            ));
        }
        full.append(line_spans);
        out.push(Line::from(full));
        *first_line = false;
    };

    for span in spans {
        let mut segment = String::new();
        let style = span.style;

        for grapheme in span.content.graphemes(true) {
            let g_width = display_width(grapheme);
            if line_width + g_width > content_width && !segment.is_empty() {
                line_spans.push(Span::styled(std::mem::take(&mut segment), style));
                push_line(&mut line_spans, out, &mut first_line);
                line_width = 0;
            } else if line_width + g_width > content_width && !line_spans.is_empty() {
                push_line(&mut line_spans, out, &mut first_line);
                line_width = 0;
            }
            segment.push_str(grapheme);
            line_width += g_width;
        }

        if !segment.is_empty() {
            line_spans.push(Span::styled(segment, style));
        }
    }

    if !line_spans.is_empty() || !prefix.is_empty() {
        push_line(&mut line_spans, out, &mut first_line);
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    wrap_text_with_options(text, width)
}

fn wrap_text_with_options(text: &str, width: usize) -> Vec<String> {
    let options = WrapOptions::new(width)
        .break_words(false)
        .word_splitter(WordSplitter::NoHyphenation);
    wrap(text, &options)
        .into_iter()
        .map(|s| s.into_owned())
        .collect()
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn pretty_tool_title(name: &str) -> String {
    name.replace('_', " ").to_uppercase()
}

fn highlight_code_line(line: &str, _lang: &str) -> Vec<Span<'static>> {
    vec![Span::styled(
        line.to_string(),
        Style::default().fg(Color::Rgb(160, 200, 255)),
    )]
}

fn decorate_cell_lines(
    lines: &mut [Line<'static>],
    selected: bool,
    matched: bool,
    tool_like: bool,
) {
    if !selected && !matched {
        return;
    }

    let bg = if selected {
        Color::Rgb(36, 42, 58)
    } else {
        Color::Rgb(28, 32, 42)
    };
    let border = if selected {
        Color::Rgb(120, 170, 255)
    } else if tool_like {
        Color::Rgb(110, 150, 110)
    } else {
        Color::Rgb(170, 150, 90)
    };

    let mut marker_set = false;
    for line in lines.iter_mut() {
        line.style = line.style.bg(bg);
        for span in &mut line.spans {
            span.style = span.style.bg(bg);
        }
        if !marker_set && !line.spans.is_empty() {
            line.spans.insert(
                0,
                Span::styled(
                    if selected { "▎" } else { "▏" },
                    Style::default().fg(border).bg(bg),
                ),
            );
            marker_set = true;
        }
    }
}

fn summarize_tool_body(body: &str, width: usize) -> String {
    let first = body
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Done");
    truncate_display_width(first.trim(), width)
}

fn truncate_display_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut used = 0usize;
    for grapheme in text.graphemes(true) {
        let grapheme_width = display_width(grapheme);
        if used + grapheme_width > width.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push_str(grapheme);
        used += grapheme_width;
    }

    if out.is_empty() {
        text.chars().take(width).collect()
    } else {
        out
    }
}
