mod markdown;
mod render;
mod tool_aggregation;

use agent_protocol::{ConversationTurn, TranscriptItem};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use textwrap::wrap;

pub use render::render_history_entry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryTone {
    User,
    Agent,
    Reasoning,
    Tool,
    Control,
    Warning,
    Error,
    Meta,
}

#[derive(Clone, Debug)]
pub struct HistoryCell {
    pub label: String,
    pub body: String,
    pub tone: HistoryTone,
    pub expanded: bool,
    pub repeat_count: usize,
    cache: std::sync::Arc<std::sync::Mutex<Option<(usize, Vec<Line<'static>>)>>>,
}

impl HistoryCell {
    pub fn new(label: impl Into<String>, body: impl Into<String>, tone: HistoryTone) -> Self {
        Self {
            label: label.into(),
            body: body.into(),
            tone,
            expanded: false,
            repeat_count: 1,
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

    pub fn append_body(&mut self, delta: &str) {
        self.body.push_str(delta);
        self.invalidate_cache();
    }

    pub fn replace_body(&mut self, body: impl Into<String>) {
        self.body = body.into();
        self.invalidate_cache();
    }

    pub fn invalidate_cache(&mut self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }

    pub fn to_lines_with_mode(&self, width: usize) -> Vec<Line<'static>> {
        if let Ok(mut cache) = self.cache.lock() {
            if let Some((w, lines)) = &*cache {
                if *w == width {
                    return lines.clone();
                }
            }
            let lines = self.render_now(width);
            *cache = Some((width, lines.clone()));
            lines
        } else {
            self.render_now(width)
        }
    }

    fn render_now(&self, width: usize) -> Vec<Line<'static>> {
        match self.tone {
            HistoryTone::User => render_user(self, width),
            HistoryTone::Agent => render_agent(self, width),
            HistoryTone::Reasoning => render_tool_like(self, width, Color::Rgb(170, 140, 255), "≈"),
            HistoryTone::Tool => render_tool_like(self, width, Color::Rgb(80, 200, 120), "◆"),
            HistoryTone::Control => render_tool_like(self, width, Color::Rgb(120, 170, 255), "•"),
            HistoryTone::Warning => render_tool_like(self, width, Color::Rgb(255, 180, 50), "◆"),
            HistoryTone::Error => render_tool_like(self, width, Color::Rgb(255, 80, 80), "◆"),
            HistoryTone::Meta => render_meta(self, width),
        }
    }
}

#[derive(Default)]
pub struct Transcript {
    cells: Vec<HistoryCell>,
}

impl Transcript {
    pub fn replace_with_history(&mut self, messages: &[TranscriptItem]) {
        self.cells.clear();
        for message in messages {
            let cell = render_history_entry(message);
            if !cell.is_empty() {
                self.cells.push(cell);
            }
        }
    }

    pub fn replace_with_turns(&mut self, turns: &[ConversationTurn]) {
        self.cells.clear();
        for turn in turns {
            for message in &turn.items {
                let cell = render_history_entry(message);
                if !cell.is_empty() {
                    self.cells.push(cell);
                }
            }
        }
    }

    pub fn push(&mut self, cell: HistoryCell) -> (usize, bool) {
        if let Some(last) = self.cells.last_mut()
            && tool_aggregation::coalesce_tool_like(last, &cell)
        {
            return (self.cells.len().saturating_sub(1), false);
        }
        self.cells.push(cell);
        (self.cells.len().saturating_sub(1), true)
    }

    pub fn replace_cells(&mut self, cells: Vec<HistoryCell>) {
        self.cells = cells;
    }

    pub fn set_tool_cells_expanded(&mut self, expanded: bool) {
        for cell in &mut self.cells {
            if matches!(
                cell.tone,
                HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.expanded = expanded;
                cell.invalidate_cache();
            }
        }
    }

    pub fn cells(&self) -> &[HistoryCell] {
        &self.cells
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn total_lines(&self, width: usize) -> usize {
        self.cells
            .iter()
            .map(|cell| cell.to_lines_with_mode(width).len())
            .sum()
    }
}

fn render_user(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(4).max(8);
    let wrapped = wrap_text(&cell.body, content_width);
    let mut lines = Vec::new();
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

fn render_agent(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let inner = width.saturating_sub(6).max(8);
    let md_lines = markdown::render_markdown(&cell.body, inner);
    let mut out = Vec::new();
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

fn render_tool_like(
    cell: &HistoryCell,
    width: usize,
    accent: Color,
    dot: &str,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let title = pretty_tool_title(&cell.label);
    let title = if cell.repeat_count > 1 {
        format!("{title} x{}", cell.repeat_count)
    } else {
        title
    };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{dot} "), Style::default().fg(accent)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(210, 215, 225))
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    let body_width = width.saturating_sub(8).max(8);
    let wrapped = wrap_text(&cell.body, body_width);
    let max_lines = if cell.expanded { 24usize } else { 2usize };
    let mut output_lines = Vec::new();
    if wrapped.len() <= max_lines {
        output_lines.extend(wrapped);
    } else {
        output_lines.extend(wrapped.iter().take(max_lines).cloned());
        output_lines.push(format!(
            "… +{} lines (ctrl+t to expand)",
            wrapped.len().saturating_sub(max_lines)
        ));
    }
    for line in output_lines {
        if !line.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                Span::styled(line, Style::default().fg(Color::Rgb(148, 152, 164))),
            ]));
        }
    }
    lines
}

fn render_meta(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let wrapped = wrap_text(&cell.body, width.saturating_sub(4).max(8));
    let mut lines = Vec::new();
    for (i, line) in wrapped.into_iter().enumerate() {
        let prefix = if i == 0 { "· " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(Color::Rgb(80, 80, 90))),
            Span::styled(line, Style::default().fg(Color::Rgb(110, 110, 120))),
        ]));
    }
    lines
}

fn pretty_tool_title(label: &str) -> String {
    match label {
        "shell_command" | "tool" => "Shell command".to_string(),
        "apply_patch" => "Edit file".to_string(),
        "fs_read_file" => "Read file".to_string(),
        "read_files" => "Read files".to_string(),
        "fuzzy_file_search" => "Find files".to_string(),
        "search_text" => "Search text".to_string(),
        "list_directory" => "List directory".to_string(),
        "fs_stat" => "File info".to_string(),
        "context" => "Context".to_string(),
        "conversation" => "conversation".to_string(),
        "reasoning" => "reasoning".to_string(),
        other => other.replace('_', " "),
    }
}

fn wrap_text(input: &str, width: usize) -> Vec<String> {
    if input.trim().is_empty() {
        return Vec::new();
    }
    wrap(input, width)
        .into_iter()
        .map(|line| line.into_owned())
        .collect()
}
