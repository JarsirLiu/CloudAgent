mod markdown;
mod render;
mod tool_aggregation;

use agent_protocol::{ConversationTurn, TranscriptItem};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use textwrap::wrap;

pub use render::{render_history_entry, RenderContext};

type RenderCache = std::sync::Arc<std::sync::Mutex<Option<(usize, Vec<Line<'static>>)>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryFormat {
    PlainText,
    Markdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKind {
    Message,
    Reasoning,
    Command,
    Tool,
    Notice,
}

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
    pub tone: HistoryTone,
    kind: HistoryKind,
    format: HistoryFormat,
    source: String,
    detail: Option<String>,
    pub expanded: bool,
    pub repeat_count: usize,
    cache: RenderCache,
}

impl HistoryCell {
    pub fn new(label: impl Into<String>, body: impl Into<String>, tone: HistoryTone) -> Self {
        Self::with_parts(
            label,
            body,
            tone,
            default_kind_for_tone(tone),
            default_format_for_tone(tone),
            None,
        )
    }

    pub fn with_format(
        label: impl Into<String>,
        body: impl Into<String>,
        tone: HistoryTone,
        format: HistoryFormat,
    ) -> Self {
        Self::with_parts(
            label,
            body,
            tone,
            default_kind_for_tone(tone),
            format,
            None,
        )
    }

    pub fn with_parts(
        label: impl Into<String>,
        body: impl Into<String>,
        tone: HistoryTone,
        kind: HistoryKind,
        format: HistoryFormat,
        detail: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            tone,
            kind,
            format,
            source: body.into(),
            detail,
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
        self.source.trim().is_empty()
    }

    pub fn append_body(&mut self, delta: &str) {
        self.source.push_str(delta);
        self.invalidate_cache();
    }

    pub fn replace_body(&mut self, body: impl Into<String>) {
        self.source = body.into();
        self.invalidate_cache();
    }

    pub fn body(&self) -> &str {
        &self.source
    }

    pub fn format(&self) -> HistoryFormat {
        self.format
    }

    pub fn kind(&self) -> HistoryKind {
        self.kind
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    pub fn invalidate_cache(&mut self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }

    pub fn to_lines_with_mode(&self, width: usize) -> Vec<Line<'static>> {
        if let Ok(mut cache) = self.cache.lock() {
            if let Some((w, lines)) = &*cache
                && *w == width
            {
                return lines.clone();
            }
            let lines = self.render_now(width);
            *cache = Some((width, lines.clone()));
            lines
        } else {
            self.render_now(width)
        }
    }

    fn render_now(&self, width: usize) -> Vec<Line<'static>> {
        match self.kind {
            HistoryKind::Message if self.tone == HistoryTone::User => render_user(self, width),
            HistoryKind::Message => render_agent(self, width),
            HistoryKind::Reasoning => render_reasoning(self, width),
            HistoryKind::Command => render_command(self, width),
            HistoryKind::Tool => render_tool_like(self, width, Color::Rgb(120, 170, 255), "•"),
            HistoryKind::Notice => render_notice(self, width),
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
        let mut context = render::RenderContext::default();
        for message in messages {
            let cell = render::render_history_entry(message, &mut context);
            if !cell.is_empty() {
                self.cells.push(cell);
            }
        }
    }

    pub fn replace_with_turns(&mut self, turns: &[ConversationTurn]) {
        self.cells.clear();
        let mut context = render::RenderContext::default();
        for turn in turns {
            for message in &turn.items {
                let cell = render_history_entry(message, &mut context);
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
    let wrapped = wrap_text(cell.body(), content_width);
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
    let md_lines = match cell.format() {
        HistoryFormat::Markdown => markdown::render_markdown(cell.body(), inner),
        HistoryFormat::PlainText => markdown::render_plaintext(cell.body(), inner),
    };
    let mut out = Vec::new();
    for (i, line) in md_lines.into_iter().enumerate() {
        let prefix = if i == 0 {
            Span::styled(" ● ", Style::default().fg(Color::Rgb(100, 180, 255)))
        } else {
            Span::raw("   ")
        };
        let mut spans = vec![prefix];
        if i == 0 && !cell.label.is_empty() {
            spans.push(Span::styled(
                format!("{}  ", cell.label),
                Style::default()
                    .fg(Color::Rgb(120, 150, 190))
                    .add_modifier(Modifier::DIM),
            ));
        }
        spans.extend(line.spans);
        out.push(Line::from(spans));
    }
    out
}

fn render_reasoning(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    render_tool_like(cell, width, Color::Rgb(170, 140, 255), "≈")
}

fn render_command(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let title = if cell.label.is_empty() {
        "Command".to_string()
    } else {
        cell.label.clone()
    };

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("› ", Style::default().fg(Color::Rgb(120, 170, 255))),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(215, 220, 232))
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    for summary in wrap_text(cell.body(), width.saturating_sub(6).max(8)) {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(summary, Style::default().fg(Color::Rgb(190, 200, 216))),
        ]));
    }

    if let Some(detail) = cell.detail() {
        for meta in wrap_text(detail, width.saturating_sub(8).max(8)) {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("↳ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                Span::styled(meta, Style::default().fg(Color::Rgb(132, 138, 150))),
            ]));
        }
    }

    lines
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
    let wrapped = wrap_text(cell.body(), body_width);
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
    let wrapped = wrap_text(cell.body(), width.saturating_sub(4).max(8));
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

fn render_notice(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.tone {
        HistoryTone::Warning => render_tool_like(cell, width, Color::Rgb(255, 180, 50), "◆"),
        HistoryTone::Error => render_tool_like(cell, width, Color::Rgb(255, 80, 80), "◆"),
        HistoryTone::Meta => render_meta(cell, width),
        _ => render_tool_like(cell, width, Color::Rgb(120, 170, 255), "•"),
    }
}

fn pretty_tool_title(label: &str) -> String {
    match label {
        "exec_command" | "tool" => "Run command".to_string(),
        "apply_patch" | "edit_file" => "Edit file".to_string(),
        "read_file" => "Read file".to_string(),
        "search_workspace" => "Search workspace".to_string(),
        "get_metadata" => "File info".to_string(),
        "read_directory" => "Read directory".to_string(),
        "create_directory" => "Create directory".to_string(),
        "write_file" => "Write file".to_string(),
        "copy_path" => "Copy path".to_string(),
        "remove_path" => "Remove path".to_string(),
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

fn default_format_for_tone(tone: HistoryTone) -> HistoryFormat {
    match tone {
        HistoryTone::Agent => HistoryFormat::Markdown,
        _ => HistoryFormat::PlainText,
    }
}

fn default_kind_for_tone(tone: HistoryTone) -> HistoryKind {
    match tone {
        HistoryTone::User | HistoryTone::Agent => HistoryKind::Message,
        HistoryTone::Reasoning => HistoryKind::Reasoning,
        HistoryTone::Tool | HistoryTone::Control => HistoryKind::Tool,
        HistoryTone::Warning | HistoryTone::Error | HistoryTone::Meta => HistoryKind::Notice,
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryCell, HistoryFormat, HistoryTone};

    fn joined(cell: &HistoryCell, width: usize) -> String {
        cell.to_lines_with_mode(width)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn agent_cells_render_markdown_tables() {
        let cell = HistoryCell::new(
            "cloudagent",
            "| 风险 | 根因 |\n| --- | --- |\n| budget | only log |",
            HistoryTone::Agent,
        );

        let rendered = joined(&cell, 100);
        assert!(rendered.contains("风险"));
        assert!(rendered.contains("根因"));
        assert!(rendered.contains(" | "));
        assert!(rendered.contains("budget"));
    }

    #[test]
    fn plaintext_cells_do_not_get_markdown_table_rendering() {
        let cell = HistoryCell::with_format(
            "tool",
            "| raw | text |",
            HistoryTone::Control,
            HistoryFormat::PlainText,
        );

        let rendered = joined(&cell, 100);
        assert!(rendered.contains("| raw | text |"));
    }
}
