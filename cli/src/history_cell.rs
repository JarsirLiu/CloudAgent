use agent_protocol::{HistoryEntry, TurnEvent};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

// ── Shimmer animation ─────────────────────────────────────────────────────────

static SHIMMER_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn elapsed() -> Duration {
    SHIMMER_START.get_or_init(Instant::now).elapsed()
}

/// Produce per-character coloured spans for a "thinking" shimmer sweep.
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

// ── Tone & cell ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryTone {
    User,
    Agent,
    Tool,
    Warning,
    Error,
    Meta,
}

#[derive(Clone, Debug)]
pub struct HistoryCell {
    label: String,
    body: String,
    tone: HistoryTone,
}

impl HistoryCell {
    pub fn new(label: impl Into<String>, body: impl Into<String>, tone: HistoryTone) -> Self {
        Self {
            label: label.into(),
            body: body.into(),
            tone,
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

    pub fn to_lines(&self, width: usize) -> Vec<Line<'static>> {
        match self.tone {
            HistoryTone::User => self.render_user(width),
            HistoryTone::Agent => self.render_agent(width),
            HistoryTone::Tool => self.render_tool_like(width, Color::Rgb(80, 200, 120), "◆"),
            HistoryTone::Warning => self.render_tool_like(width, Color::Rgb(255, 180, 50), "◆"),
            HistoryTone::Error => self.render_tool_like(width, Color::Rgb(255, 80, 80), "◆"),
            HistoryTone::Meta => self.render_meta(width),
        }
    }

    // ── User bubble (right-aligned feel, subtle bg) ───────────────────────────
    fn render_user(&self, width: usize) -> Vec<Line<'static>> {
        let content_width = width.saturating_sub(6).max(8);
        let wrapped = wrap_text(&self.body, content_width);
        let mut lines = vec![Line::raw("")];
        for text in wrapped {
            let padding = " ".repeat(width.saturating_sub(display_width(&text) + 4));
            lines.push(Line::from(vec![
                Span::styled(
                    padding,
                    Style::default(),
                ),
                Span::styled(
                    "▌ ",
                    Style::default().fg(Color::Rgb(100, 120, 200)),
                ),
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

    // ── Agent message — full Markdown render ──────────────────────────────────
    fn render_agent(&self, width: usize) -> Vec<Line<'static>> {
        let inner = width.saturating_sub(4).max(8);
        let md_lines = render_markdown(&self.body, inner);
        let mut out = vec![Line::raw("")];
        for (i, line) in md_lines.into_iter().enumerate() {
            let prefix = if i == 0 {
                Span::styled("● ", Style::default().fg(Color::Rgb(100, 180, 255)))
            } else {
                Span::raw("  ")
            };
            let mut spans = vec![prefix];
            spans.extend(line.spans);
            out.push(Line::from(spans));
        }
        out
    }

    // ── Tool / Warning / Error row ────────────────────────────────────────────
    fn render_tool_like(&self, width: usize, accent: Color, dot: &str) -> Vec<Line<'static>> {
        let mut lines = vec![Line::raw("")];
        let title = pretty_tool_title(&self.label);
        lines.push(Line::from(vec![
            Span::styled(format!("{dot} "), Style::default().fg(accent)),
            Span::styled(
                title,
                Style::default()
                    .fg(Color::Rgb(220, 220, 220))
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        let body_width = width.saturating_sub(6).max(8);
        for (i, line) in wrap_text(&self.body, body_width).into_iter().enumerate() {
            let branch = if i == 0 { "└ " } else { "  " };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(branch, Style::default().fg(Color::Rgb(80, 80, 90))),
                Span::styled(line, Style::default().fg(Color::Rgb(160, 160, 170))),
            ]));
        }
        lines
    }

    // ── Meta (session events, info) ───────────────────────────────────────────
    fn render_meta(&self, width: usize) -> Vec<Line<'static>> {
        let wrapped = wrap_text(&self.body, width.saturating_sub(4).max(8));
        let mut lines = vec![Line::raw("")];
        for (i, line) in wrapped.into_iter().enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled("· ", Style::default().fg(Color::Rgb(80, 80, 90))),
                    Span::styled(line, Style::default().fg(Color::Rgb(110, 110, 120))),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::Rgb(110, 110, 120)),
                )));
            }
        }
        lines
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

    pub fn push(&mut self, cell: HistoryCell) {
        if cell.is_empty() {
            return;
        }
        self.cells.push(cell);
        if self.cells.len() > 500 {
            let excess = self.cells.len() - 500;
            self.cells.drain(0..excess);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn render_lines(
        &self,
        width: usize,
        height: usize,
        scroll_offset: usize,
    ) -> Vec<Line<'static>> {
        let mut all_lines = Vec::new();
        for cell in &self.cells {
            all_lines.extend(cell.to_lines(width));
        }
        let total = all_lines.len();
        let end = total.saturating_sub(scroll_offset);
        let start = end.saturating_sub(height);
        all_lines.into_iter().skip(start).take(end - start).collect()
    }

    pub fn total_lines(&self, width: usize) -> usize {
        self.cells.iter().flat_map(|c| c.to_lines(width)).count()
    }
}

// ── Event → cell mapping ──────────────────────────────────────────────────────

pub struct TurnRender {
    pub log: Option<HistoryCell>,
    pub status: Option<String>,
}

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
                    "Working with tools".to_string()
                } else {
                    String::new()
                }
            });
            HistoryCell::from_message("cloudagent", body, HistoryTone::Agent)
        }
        HistoryEntry::Tool { name, content, .. } => {
            HistoryCell::from_message(name.clone(), content.clone(), HistoryTone::Tool)
        }
    }
}

pub fn render_turn_event(event: &TurnEvent) -> TurnRender {
    match event {
        TurnEvent::TurnStarted { .. } => TurnRender {
            log: None,
            status: Some("Working".to_string()),
        },
        TurnEvent::ModelRequestStarted { .. } => TurnRender {
            log: None,
            status: Some("Thinking".to_string()),
        },
        TurnEvent::ModelResponseReceived {
            tool_call_count, ..
        } => TurnRender {
            log: None,
            status: Some(if *tool_call_count > 0 {
                "Planning tool work".to_string()
            } else {
                "Drafting response".to_string()
            }),
        },
        TurnEvent::AssistantMessage { content, .. } => TurnRender {
            log: Some(HistoryCell::from_message(
                "cloudagent",
                content.clone(),
                HistoryTone::Agent,
            )),
            status: Some("Responding".to_string()),
        },
        TurnEvent::ToolCallRequested { call, .. } => TurnRender {
            log: Some(HistoryCell::from_message(
                format_tool_call_label(&call.name, &call.arguments.to_string()),
                "Started".to_string(),
                HistoryTone::Tool,
            )),
            status: Some(format!("Running {}", call.name)),
        },
        TurnEvent::ApprovalRequested { request, .. } => TurnRender {
            log: Some(HistoryCell::from_message(
                format!("Edit {}", request.tool_name),
                format!("{} | {}", request.reason, request.arguments_preview),
                HistoryTone::Warning,
            )),
            status: Some(format!("Needs approval for {}", request.tool_name)),
        },
        TurnEvent::ApprovalResolved {
            tool_call_id,
            approved,
            ..
        } => TurnRender {
            log: Some(HistoryCell::from_message(
                "Approval".to_string(),
                if *approved {
                    format!("Approved {tool_call_id}")
                } else {
                    format!("Denied {tool_call_id}")
                },
                if *approved {
                    HistoryTone::Meta
                } else {
                    HistoryTone::Warning
                },
            )),
            status: Some(if *approved {
                "Approval granted".to_string()
            } else {
                "Approval denied".to_string()
            }),
        },
        TurnEvent::ToolCallCompleted { result, .. } => TurnRender {
            log: Some(HistoryCell::from_message(
                pretty_tool_title(&result.name),
                result.summary.clone(),
                HistoryTone::Tool,
            )),
            status: Some(format!("Finished {}", result.name)),
        },
        TurnEvent::ToolCallFailed {
            tool_name, error, ..
        } => TurnRender {
            log: Some(HistoryCell::from_message(
                pretty_tool_title(tool_name),
                error.clone(),
                HistoryTone::Error,
            )),
            status: Some(format!("{} failed", tool_name)),
        },
        TurnEvent::TurnCompleted { .. } => TurnRender {
            log: None,
            status: Some("Done".to_string()),
        },
        TurnEvent::TurnFailed { error, .. } => TurnRender {
            log: Some(HistoryCell::from_message(
                "Turn".to_string(),
                error.clone(),
                HistoryTone::Error,
            )),
            status: Some("Turn failed".to_string()),
        },
        TurnEvent::TurnCancelled { reason, .. } => TurnRender {
            log: Some(HistoryCell::from_message(
                "Turn".to_string(),
                reason.clone(),
                HistoryTone::Warning,
            )),
            status: Some("Turn cancelled".to_string()),
        },
    }
}

// ── Markdown renderer (pulldown-cmark → ratatui Lines) ───────────────────────

fn render_markdown(input: &str, width: usize) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(input, opts);

    let mut out: Vec<Line<'static>> = Vec::new();
    // current line being built
    let mut current: Vec<Span<'static>> = Vec::new();
    // inline style stack
    let mut style_stack: Vec<Style> = vec![Style::default().fg(Color::Rgb(220, 220, 220))];
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();
    let mut list_depth: usize = 0;
    let mut ordered_counters: Vec<u64> = Vec::new();

    let flush = |current: &mut Vec<Span<'static>>, out: &mut Vec<Line<'static>>, width: usize| {
        if current.is_empty() {
            out.push(Line::raw(""));
        } else {
            let flat: String = current.iter().map(|s| s.content.as_ref()).collect();
            let wrapped = wrap_text(&flat, width);
            // Re-apply the last span's style to all wrapped lines (simple approach)
            let style = current.last().map(|s| s.style).unwrap_or_default();
            for (i, wl) in wrapped.into_iter().enumerate() {
                if i == 0 {
                    out.push(Line::from(Span::styled(wl, style)));
                } else {
                    out.push(Line::from(Span::styled(format!("  {wl}"), style)));
                }
            }
            current.clear();
        }
    };

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(&mut current, &mut out, width);
                in_code_block = true;
                code_lang = match &kind {
                    CodeBlockKind::Fenced(lang) => lang.split([' ', ',']).next().unwrap_or("").to_string(),
                    _ => String::new(),
                };
                code_buf.clear();
                out.push(Line::from(Span::styled(
                    format!(" {} ", if code_lang.is_empty() { "code" } else { &code_lang }),
                    Style::default()
                        .fg(Color::Rgb(100, 140, 200))
                        .bg(Color::Rgb(28, 35, 48))
                        .add_modifier(Modifier::BOLD),
                )));
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                for line in code_buf.lines() {
                    let padded = format!("  {line}");
                    let vis: usize = padded.chars().map(|c| c.width().unwrap_or(0)).sum();
                    let fill = if vis < width { " ".repeat(width - vis) } else { String::new() };
                    out.push(Line::from(Span::styled(
                        format!("{padded}{fill}"),
                        Style::default()
                            .fg(Color::Rgb(180, 210, 255))
                            .bg(Color::Rgb(22, 28, 40)),
                    )));
                }
                code_buf.clear();
                code_lang.clear();
                out.push(Line::raw(""));
            }
            Event::Text(text) => {
                if in_code_block {
                    code_buf.push_str(&text);
                } else {
                    let style = *style_stack.last().unwrap_or(&Style::default());
                    for (i, part) in text.split('\n').enumerate() {
                        if i > 0 {
                            flush(&mut current, &mut out, width);
                        }
                        if !part.is_empty() {
                            current.push(Span::styled(part.to_string(), style));
                        }
                    }
                }
            }
            Event::Code(code) => {
                current.push(Span::styled(
                    format!("`{code}`"),
                    Style::default().fg(Color::Rgb(130, 200, 130)),
                ));
            }
            Event::Start(Tag::Strong) => {
                let base = *style_stack.last().unwrap_or(&Style::default());
                style_stack.push(base.add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::Strong) => { style_stack.pop(); }
            Event::Start(Tag::Emphasis) => {
                let base = *style_stack.last().unwrap_or(&Style::default());
                style_stack.push(base.add_modifier(Modifier::ITALIC));
            }
            Event::End(TagEnd::Emphasis) => { style_stack.pop(); }
            Event::Start(Tag::Strikethrough) => {
                let base = *style_stack.last().unwrap_or(&Style::default());
                style_stack.push(base.add_modifier(Modifier::CROSSED_OUT));
            }
            Event::End(TagEnd::Strikethrough) => { style_stack.pop(); }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut current, &mut out, width);
                let prefix = "#".repeat(level as usize);
                let style = Style::default()
                    .fg(Color::Rgb(130, 180, 255))
                    .add_modifier(Modifier::BOLD);
                style_stack.push(style);
                current.push(Span::styled(format!("{prefix} "), style));
            }
            Event::End(TagEnd::Heading(_)) => {
                style_stack.pop();
                flush(&mut current, &mut out, width);
                out.push(Line::raw(""));
            }
            Event::Start(Tag::Paragraph) => {
                flush(&mut current, &mut out, width);
            }
            Event::End(TagEnd::Paragraph) => {
                flush(&mut current, &mut out, width);
                out.push(Line::raw(""));
            }
            Event::Start(Tag::List(start)) => {
                list_depth += 1;
                ordered_counters.push(start.unwrap_or(0));
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                ordered_counters.pop();
                if list_depth == 0 {
                    out.push(Line::raw(""));
                }
            }
            Event::Start(Tag::Item) => {
                flush(&mut current, &mut out, width);
                let indent = "  ".repeat(list_depth.saturating_sub(1));
                let marker = if let Some(n) = ordered_counters.last_mut() {
                    *n += 1;
                    let m = format!("{}{}. ", indent, *n - 1);
                    Span::styled(m, Style::default().fg(Color::Rgb(130, 160, 230)))
                } else {
                    Span::styled(
                        format!("{}• ", indent),
                        Style::default().fg(Color::Rgb(130, 160, 230)),
                    )
                };
                current.push(marker);
            }
            Event::End(TagEnd::Item) => {
                flush(&mut current, &mut out, width);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut current, &mut out, width);
                let base = *style_stack.last().unwrap_or(&Style::default());
                style_stack.push(base.fg(Color::Rgb(150, 150, 160)));
                current.push(Span::styled("▎ ", Style::default().fg(Color::Rgb(80, 80, 100))));
            }
            Event::End(TagEnd::BlockQuote) => {
                style_stack.pop();
                flush(&mut current, &mut out, width);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let style = Style::default()
                    .fg(Color::Rgb(100, 160, 255))
                    .add_modifier(Modifier::UNDERLINED);
                style_stack.push(style);
                let _ = dest_url;
            }
            Event::End(TagEnd::Link) => {
                style_stack.pop();
            }
            Event::Rule => {
                flush(&mut current, &mut out, width);
                out.push(Line::from(Span::styled(
                    "─".repeat(width.min(60)),
                    Style::default().fg(Color::Rgb(60, 60, 70)),
                )));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush(&mut current, &mut out, width);
            }
            _ => {}
        }
    }

    flush(&mut current, &mut out, width);

    // Remove trailing blank lines
    while out.last().map(|l: &Line| l.spans.is_empty() || l.spans.iter().all(|s| s.content.trim().is_empty())).unwrap_or(false) {
        out.pop();
    }

    if out.is_empty() {
        out.push(Line::from(Span::styled(
            "",
            Style::default().fg(Color::Rgb(220, 220, 220)),
        )));
    }
    out
}

// ── Text utilities ────────────────────────────────────────────────────────────

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        // Use textwrap for word-boundary wrapping
        for wl in textwrap::wrap(paragraph, width) {
            lines.push(wl.into_owned());
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

// ── Label helpers ─────────────────────────────────────────────────────────────

fn pretty_tool_title(name: &str) -> String {
    match name {
        "shell_command" => "Shell command".to_string(),
        "read_file" => "Read file".to_string(),
        "write_file" => "Edit file".to_string(),
        "list_dir" => "List directory".to_string(),
        other => {
            let s = other.replace('_', " ");
            let mut c = s.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        }
    }
}

fn format_tool_call_label(name: &str, args: &str) -> String {
    let title = pretty_tool_title(name);
    let compact = args.replace('\n', " ");
    let compact = compact.trim();
    if compact.is_empty() || compact == "{}" {
        title
    } else {
        let shortened = if compact.chars().count() > 42 {
            let prefix: String = compact.chars().take(42).collect();
            format!("{prefix}…")
        } else {
            compact.to_string()
        };
        format!("{title}({shortened})")
    }
}
