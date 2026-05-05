mod markdown;
mod render;
mod tool_aggregation;

use agent_protocol::{ConversationTurn, TranscriptItem};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use textwrap::wrap;

pub use render::{RenderContext, render_active_control_placeholder, render_history_entry};

type RenderCache = std::sync::Arc<std::sync::Mutex<Option<(usize, Vec<Line<'static>>)>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryFormat {
    PlainText,
    Markdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserCell {
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentCell {
    pub label: String,
    pub text: String,
    pub format: HistoryFormat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasoningCell {
    pub label: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorationCell {
    pub label: String,
    pub summary: String,
    pub aggregate: ExplorationAggregate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecCell {
    pub label: String,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditCell {
    pub label: String,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoCell {
    pub label: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryKind {
    Message,
    Reasoning,
    Exploration,
    Command,
    Tool,
    Notice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorationAggregate {
    pub read_files: usize,
    pub searches: usize,
    pub inspect_commands: usize,
    pub listed_directories: usize,
    pub metadata_reads: usize,
    pub details: Vec<String>,
}

impl ExplorationAggregate {
    pub fn new(detail: String) -> Self {
        Self {
            read_files: 0,
            searches: 0,
            inspect_commands: 0,
            listed_directories: 0,
            metadata_reads: 0,
            details: vec![detail],
        }
    }

    pub fn push_detail(&mut self, detail: String) {
        if !detail.trim().is_empty() {
            self.details.push(detail);
        }
    }
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
    pub tone: HistoryTone,
    content: HistoryContent,
    pub expanded: bool,
    pub repeat_count: usize,
    cache: RenderCache,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HistoryContent {
    User(UserCell),
    Agent(AgentCell),
    Reasoning(ReasoningCell),
    Exploration(ExplorationCell),
    Exec(ExecCell),
    Edit(EditCell),
    Info(InfoCell),
}

impl HistoryCell {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            tone: HistoryTone::User,
            content: HistoryContent::User(UserCell { text: text.into() }),
            expanded: false,
            repeat_count: 1,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn agent(label: impl Into<String>, text: impl Into<String>, format: HistoryFormat) -> Self {
        Self {
            tone: HistoryTone::Agent,
            content: HistoryContent::Agent(AgentCell {
                label: label.into(),
                text: text.into(),
                format,
            }),
            expanded: false,
            repeat_count: 1,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn reasoning(label: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tone: HistoryTone::Reasoning,
            content: HistoryContent::Reasoning(ReasoningCell {
                label: label.into(),
                text: text.into(),
            }),
            expanded: false,
            repeat_count: 1,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn exploration(
        label: impl Into<String>,
        summary: impl Into<String>,
        aggregate: ExplorationAggregate,
        tone: HistoryTone,
    ) -> Self {
        Self {
            tone,
            content: HistoryContent::Exploration(ExplorationCell {
                label: label.into(),
                summary: summary.into(),
                aggregate,
            }),
            expanded: false,
            repeat_count: 1,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn exec(
        label: impl Into<String>,
        summary: impl Into<String>,
        detail: Option<String>,
        tone: HistoryTone,
    ) -> Self {
        Self {
            tone,
            content: HistoryContent::Exec(ExecCell {
                label: label.into(),
                summary: summary.into(),
                detail,
            }),
            expanded: false,
            repeat_count: 1,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn edit(
        label: impl Into<String>,
        summary: impl Into<String>,
        detail: Option<String>,
        tone: HistoryTone,
    ) -> Self {
        Self {
            tone,
            content: HistoryContent::Edit(EditCell {
                label: label.into(),
                summary: summary.into(),
                detail,
            }),
            expanded: false,
            repeat_count: 1,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn info(label: impl Into<String>, text: impl Into<String>, tone: HistoryTone) -> Self {
        Self {
            tone,
            content: HistoryContent::Info(InfoCell {
                label: label.into(),
                text: text.into(),
            }),
            expanded: false,
            repeat_count: 1,
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.body().trim().is_empty()
    }

    pub fn append_body(&mut self, delta: &str) {
        match &mut self.content {
            HistoryContent::Agent(cell) => cell.text.push_str(delta),
            HistoryContent::Reasoning(cell) => cell.text.push_str(delta),
            HistoryContent::Info(cell) => cell.text.push_str(delta),
            HistoryContent::User(cell) => cell.text.push_str(delta),
            HistoryContent::Exploration(cell) => cell.summary.push_str(delta),
            HistoryContent::Exec(cell) => cell.summary.push_str(delta),
            HistoryContent::Edit(cell) => cell.summary.push_str(delta),
        }
        self.invalidate_cache();
    }

    pub fn replace_body(&mut self, body: impl Into<String>) {
        let body = body.into();
        match &mut self.content {
            HistoryContent::Agent(cell) => cell.text = body,
            HistoryContent::Reasoning(cell) => cell.text = body,
            HistoryContent::Info(cell) => cell.text = body,
            HistoryContent::User(cell) => cell.text = body,
            HistoryContent::Exploration(cell) => cell.summary = body,
            HistoryContent::Exec(cell) => cell.summary = body,
            HistoryContent::Edit(cell) => cell.summary = body,
        }
        self.invalidate_cache();
    }

    pub fn body(&self) -> &str {
        match &self.content {
            HistoryContent::User(cell) => &cell.text,
            HistoryContent::Agent(cell) => &cell.text,
            HistoryContent::Reasoning(cell) => &cell.text,
            HistoryContent::Exploration(cell) => &cell.summary,
            HistoryContent::Exec(cell) => &cell.summary,
            HistoryContent::Edit(cell) => &cell.summary,
            HistoryContent::Info(cell) => &cell.text,
        }
    }

    pub fn format(&self) -> HistoryFormat {
        match &self.content {
            HistoryContent::Agent(cell) => cell.format,
            _ => HistoryFormat::PlainText,
        }
    }

    pub fn kind(&self) -> HistoryKind {
        match self.content {
            HistoryContent::User(_) | HistoryContent::Agent(_) => HistoryKind::Message,
            HistoryContent::Reasoning(_) => HistoryKind::Reasoning,
            HistoryContent::Exploration(_) => HistoryKind::Exploration,
            HistoryContent::Exec(_) => HistoryKind::Command,
            HistoryContent::Edit(_) => HistoryKind::Tool,
            HistoryContent::Info(_) => default_kind_for_tone(self.tone),
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match &self.content {
            HistoryContent::Exec(cell) => cell.detail.as_deref(),
            HistoryContent::Edit(cell) => cell.detail.as_deref(),
            _ => None,
        }
    }

    pub fn aggregate(&self) -> Option<&ExplorationAggregate> {
        match &self.content {
            HistoryContent::Exploration(cell) => Some(&cell.aggregate),
            _ => None,
        }
    }

    pub fn set_aggregate(&mut self, aggregate: ExplorationAggregate) {
        if let HistoryContent::Exploration(cell) = &mut self.content {
            cell.aggregate = aggregate;
        }
        self.invalidate_cache();
    }

    pub fn set_summary(&mut self, summary: impl Into<String>) {
        let summary = summary.into();
        match &mut self.content {
            HistoryContent::Exploration(cell) => cell.summary = summary,
            HistoryContent::Exec(cell) => cell.summary = summary,
            HistoryContent::Edit(cell) => cell.summary = summary,
            _ => {}
        }
        self.invalidate_cache();
    }

    pub fn label(&self) -> &str {
        match &self.content {
            HistoryContent::User(_) => "you",
            HistoryContent::Agent(cell) => &cell.label,
            HistoryContent::Reasoning(cell) => &cell.label,
            HistoryContent::Exploration(cell) => &cell.label,
            HistoryContent::Exec(cell) => &cell.label,
            HistoryContent::Edit(cell) => &cell.label,
            HistoryContent::Info(cell) => &cell.label,
        }
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
        match self.kind() {
            HistoryKind::Message if self.tone == HistoryTone::User => render_user(self, width),
            HistoryKind::Message => render_agent(self, width),
            HistoryKind::Reasoning => render_reasoning(self, width),
            HistoryKind::Exploration => render_exploration(self, width),
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
        let mut context = render::RenderContext;
        for message in messages {
            let cell = render::render_history_entry(message, &mut context);
            if !cell.is_empty() {
                let _ = self.push_aggregated(cell);
            }
        }
    }

    pub fn replace_with_turns(&mut self, turns: &[ConversationTurn]) {
        self.cells.clear();
        let mut context = render::RenderContext;
        for turn in turns {
            for message in &turn.items {
                let cell = render_history_entry(message, &mut context);
                if !cell.is_empty() {
                    let _ = self.push_aggregated(cell);
                }
            }
        }
    }

    pub fn push_live(&mut self, cell: HistoryCell) -> (usize, bool) {
        self.push_with_policy(cell, false)
    }

    pub fn push_aggregated(&mut self, cell: HistoryCell) -> (usize, bool) {
        self.push_with_policy(cell, true)
    }

    fn push_with_policy(&mut self, cell: HistoryCell, allow_exploration: bool) -> (usize, bool) {
        if let Some(last) = self.cells.last_mut()
            && tool_aggregation::coalesce_tool_like(last, &cell, allow_exploration)
        {
            return (self.cells.len().saturating_sub(1), false);
        }
        self.cells.push(cell);
        (self.cells.len().saturating_sub(1), true)
    }

    pub fn replace_cells(&mut self, cells: Vec<HistoryCell>) {
        self.cells.clear();
        for cell in cells {
            let _ = self.push_aggregated(cell);
        }
    }

    pub fn consolidate_trailing_exploration_run(&mut self) -> bool {
        let end = self.cells.len();
        let mut start = end;
        while start > 0 && self.cells[start - 1].kind() == HistoryKind::Exploration {
            start -= 1;
        }
        if end.saturating_sub(start) <= 1 {
            return false;
        }

        let mut merged = self.cells[start].clone();
        for cell in &self.cells[start + 1..end] {
            if !tool_aggregation::coalesce_tool_like(&mut merged, cell, true) {
                return false;
            }
        }
        self.cells.splice(start..end, std::iter::once(merged));
        true
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
        if i == 0 && !cell.label().is_empty() {
            spans.push(Span::styled(
                format!("{}  ", cell.label()),
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
    let inner = width.saturating_sub(8).max(8);
    let text_lines = markdown::render_plaintext(cell.body(), inner);
    let mut out = Vec::new();
    for (i, line) in text_lines.into_iter().enumerate() {
        let mut spans = Vec::new();
        if i == 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "≈ ",
                Style::default().fg(Color::Rgb(170, 140, 255)),
            ));
            if !cell.label().is_empty() {
                spans.push(Span::styled(
                    format!("{}  ", cell.label()),
                    Style::default()
                        .fg(Color::Rgb(210, 215, 225))
                        .add_modifier(Modifier::BOLD),
                ));
            }
        } else {
            spans.push(Span::raw("    "));
            spans.push(Span::styled(
                "│ ",
                Style::default().fg(Color::Rgb(90, 96, 108)),
            ));
        }
        spans.extend(line.spans);
        out.push(Line::from(spans));
    }
    out
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
    let max_details = if cell.expanded { 8 } else { 2 };
    let mut inline = details
        .iter()
        .take(max_details)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if details.len() > max_details {
        if !inline.is_empty() {
            inline.push_str("; ");
        }
        inline.push_str(&format!(
            "… +{} more",
            details.len().saturating_sub(max_details)
        ));
    }

    let mut text = cell.body().to_string();
    if !inline.is_empty() {
        text.push_str(" — ");
        text.push_str(&inline);
    }

    let available = width.saturating_sub(6).max(12);
    let wrapped = wrap_text(&text, available);
    let mut lines = Vec::new();
    for (index, segment) in wrapped.into_iter().enumerate() {
        let mut spans = Vec::new();
        if index == 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "◦ ",
                Style::default().fg(Color::Rgb(120, 170, 255)),
            ));
            spans.push(Span::styled(
                format!("{title}  "),
                Style::default()
                    .fg(Color::Rgb(215, 220, 232))
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw("    "));
        }
        spans.push(Span::styled(
            segment,
            Style::default().fg(Color::Rgb(190, 200, 216)),
        ));
        lines.push(Line::from(spans));
    }
    lines
}

fn render_command(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let title = if cell.label().is_empty() {
        "Command".to_string()
    } else {
        cell.label().to_string()
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
    let title = pretty_tool_title(cell.label());
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
            "… +{} lines (ctrl+t to toggle details)",
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
    use super::{HistoryCell, HistoryFormat};

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
        let cell = HistoryCell::agent(
            "cloudagent",
            "| 风险 | 根因 |\n| --- | --- |\n| budget | only log |",
            HistoryFormat::Markdown,
        );

        let rendered = joined(&cell, 100);
        assert!(rendered.contains("风险"));
        assert!(rendered.contains("根因"));
        assert!(rendered.contains(" | "));
        assert!(rendered.contains("budget"));
    }

    #[test]
    fn plaintext_cells_do_not_get_markdown_table_rendering() {
        let cell = HistoryCell::agent("tool", "| raw | text |", HistoryFormat::PlainText);

        let rendered = joined(&cell, 100);
        assert!(rendered.contains("| raw | text |"));
    }
}
