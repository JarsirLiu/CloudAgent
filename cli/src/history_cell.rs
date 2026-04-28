use agent_protocol::{HistoryEntry, TurnEvent};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

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
            HistoryTone::Tool => self.render_tool_like(width, Color::Green, "●"),
            HistoryTone::Warning => self.render_tool_like(width, Color::LightRed, "●"),
            HistoryTone::Error => self.render_tool_like(width, Color::Red, "●"),
            HistoryTone::Meta => self.render_meta(width),
        }
    }

    fn render_user(&self, width: usize) -> Vec<Line<'static>> {
        let content_width = width.saturating_sub(4).clamp(8, width);
        let wrapped = wrap_text(&self.body, content_width.saturating_sub(2));
        let mut lines = vec![Line::raw("")];
        for line in wrapped {
            let text = format!("> {}", line);
            lines.push(padded_bg_line(
                text,
                content_width,
                Style::default().fg(Color::White).bg(Color::Rgb(46, 46, 46)),
            ));
        }
        lines
    }

    fn render_agent(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = vec![Line::raw("")];
        let rendered = render_markdownish_blocks(&self.body, width.saturating_sub(4).max(8));
        for (index, line) in rendered.into_iter().enumerate() {
            let prefix = if index == 0 { "● " } else { "  " };
            let mut spans = vec![Span::styled(prefix, Style::default().fg(Color::White))];
            spans.extend(line.spans);
            lines.push(Line::from(spans));
        }
        lines
    }

    fn render_tool_like(&self, width: usize, accent: Color, dot: &str) -> Vec<Line<'static>> {
        let mut lines = vec![Line::raw("")];
        let title = pretty_tool_title(&self.label);
        lines.push(Line::from(vec![
            Span::styled(format!("{dot} "), Style::default().fg(accent)),
            Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        let wrapped = wrap_text(&self.body, width.saturating_sub(6).max(8));
        for (index, line) in wrapped.into_iter().enumerate() {
            let branch = if index == 0 { "└ " } else { "  " };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(branch, Style::default().fg(Color::Gray)),
                Span::styled(line, Style::default().fg(Color::Gray)),
            ]));
        }
        lines
    }

    fn render_meta(&self, width: usize) -> Vec<Line<'static>> {
        let wrapped = wrap_text(&self.body, width.saturating_sub(4).max(8));
        let mut lines = vec![Line::raw("")];
        for (index, line) in wrapped.into_iter().enumerate() {
            if index == 0 {
                lines.push(Line::from(vec![
                    Span::styled("· ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line, Style::default().fg(Color::DarkGray)),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        lines
    }
}

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
        all_lines
            .into_iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect()
    }

    pub fn total_lines(&self, width: usize) -> usize {
        self.cells
            .iter()
            .flat_map(|cell| cell.to_lines(width))
            .count()
    }
}

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

fn padded_bg_line(text: String, width: usize, style: Style) -> Line<'static> {
    let mut content = text;
    let visual: usize = content.chars().map(|c| c.width().unwrap_or(0)).sum();
    if visual < width {
        content.push_str(&" ".repeat(width - visual));
    }
    Line::from(Span::styled(content, style))
}

fn pretty_tool_title(name: &str) -> String {
    match name {
        "shell_command" => "Shell command".to_string(),
        "read_file" => "Read file".to_string(),
        "write_file" => "Edit file".to_string(),
        "list_dir" => "List directory".to_string(),
        other => other.replace('_', " "),
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
            format!("{prefix}...")
        } else {
            compact.to_string()
        };
        format!("{title}({shortened})")
    }
}

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

        let mut current = String::new();
        let mut current_width = 0usize;
        for ch in paragraph.chars() {
            let ch_width = ch.width().unwrap_or(0);
            // 如果加上这个字符会超出宽度，先换行
            if current_width + ch_width > width && !current.is_empty() {
                lines.push(current);
                current = String::new();
                current_width = 0;
            }
            current.push(ch);
            current_width += ch_width;
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn render_markdownish_blocks(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in text.lines() {
        let trimmed = raw.trim_end();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            if in_code {
                out.push(Line::from(Span::styled(
                    "code",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                )));
            }
            continue;
        }

        let style = if in_code {
            Style::default()
                .fg(Color::Rgb(210, 235, 255))
                .bg(Color::Rgb(25, 34, 46))
        } else {
            Style::default().fg(Color::White)
        };

        let normalized = if !in_code && trimmed.starts_with("- ") {
            format!("• {}", trimmed.trim_start_matches("- "))
        } else {
            trimmed.to_string()
        };

        let wrapped = if normalized.is_empty() {
            vec![String::new()]
        } else {
            wrap_text(&normalized, width)
        };

        for line in wrapped {
            if in_code {
                let mut padded = format!("  {line}");
                let visual: usize = padded.chars().map(|c| c.width().unwrap_or(0)).sum();
                if visual < width {
                    padded.push_str(&" ".repeat(width - visual));
                }
                out.push(Line::from(Span::styled(padded, style)));
            } else {
                out.push(Line::from(Span::styled(line, style)));
            }
        }
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(
            "",
            Style::default().fg(Color::White),
        )));
    }
    out
}
