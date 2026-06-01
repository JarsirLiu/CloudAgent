mod markdown;
mod render;
pub(crate) mod tool_aggregation;
mod wrapping;

use agent_core::{ConversationTurn, TranscriptItem};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use wrapping::{WrapOptions, word_wrap_text};

pub(crate) use render::{
    RenderContext, humanize_tool_label, render_active_item_placeholder, render_history_entry,
};

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
    pub presentation: ReasoningPresentation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasoningPresentation {
    Detailed,
    Summary,
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
pub struct ToolGroupCell {
    pub label: String,
    pub summary: String,
    pub children: Vec<HistoryCell>,
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
    view: HistoryCellView,
    cache: RenderCache,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryCellView {
    expanded: bool,
    repeat_count: usize,
    stream_continuation: bool,
    provisional_stream: bool,
    stream_item_id: Option<String>,
}

impl Default for HistoryCellView {
    fn default() -> Self {
        Self {
            expanded: false,
            repeat_count: 1,
            stream_continuation: false,
            provisional_stream: false,
            stream_item_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HistoryContent {
    User(UserCell),
    Agent(AgentCell),
    Reasoning(ReasoningCell),
    Exploration(ExplorationCell),
    Exec(ExecCell),
    Edit(EditCell),
    ToolGroup(ToolGroupCell),
    Info(InfoCell),
}

impl HistoryCell {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            tone: HistoryTone::User,
            content: HistoryContent::User(UserCell { text: text.into() }),
            view: HistoryCellView::default(),
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
            view: HistoryCellView::default(),
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn reasoning(label: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tone: HistoryTone::Reasoning,
            content: HistoryContent::Reasoning(ReasoningCell {
                label: label.into(),
                text: text.into(),
                presentation: ReasoningPresentation::Detailed,
            }),
            view: HistoryCellView::default(),
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn reasoning_summary(label: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            tone: HistoryTone::Reasoning,
            content: HistoryContent::Reasoning(ReasoningCell {
                label: label.into(),
                text: text.into(),
                presentation: ReasoningPresentation::Summary,
            }),
            view: HistoryCellView::default(),
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
            view: HistoryCellView::default(),
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
            view: HistoryCellView::default(),
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
            view: HistoryCellView::default(),
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
            view: HistoryCellView::default(),
            cache: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn tool_group(
        label: impl Into<String>,
        summary: impl Into<String>,
        children: Vec<HistoryCell>,
        tone: HistoryTone,
    ) -> Self {
        Self {
            tone,
            content: HistoryContent::ToolGroup(ToolGroupCell {
                label: label.into(),
                summary: summary.into(),
                children,
            }),
            view: HistoryCellView::default(),
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
            HistoryContent::ToolGroup(cell) => cell.summary.push_str(delta),
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
            HistoryContent::ToolGroup(cell) => cell.summary = body,
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
            HistoryContent::ToolGroup(cell) => &cell.summary,
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
            HistoryContent::Edit(_) | HistoryContent::ToolGroup(_) => HistoryKind::Tool,
            HistoryContent::Info(_) => default_kind_for_tone(self.tone),
        }
    }

    pub fn is_stream_continuation(&self) -> bool {
        self.view.stream_continuation
    }

    pub fn set_stream_continuation(&mut self, is_stream_continuation: bool) {
        self.view.stream_continuation = is_stream_continuation;
    }

    pub fn with_stream_continuation(mut self, is_stream_continuation: bool) -> Self {
        self.view.stream_continuation = is_stream_continuation;
        self
    }

    pub fn is_provisional_stream(&self) -> bool {
        self.view.provisional_stream
    }

    pub fn set_provisional_stream(&mut self, provisional_stream: bool) {
        self.view.provisional_stream = provisional_stream;
    }

    pub fn with_provisional_stream(mut self, provisional_stream: bool) -> Self {
        self.view.provisional_stream = provisional_stream;
        self
    }

    pub fn stream_item_id(&self) -> Option<&str> {
        self.view.stream_item_id.as_deref()
    }

    pub fn with_stream_item_id(mut self, item_id: impl Into<String>) -> Self {
        self.view.stream_item_id = Some(item_id.into());
        self
    }

    pub fn is_expanded(&self) -> bool {
        self.view.expanded
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.view.expanded = expanded;
        self.invalidate_cache();
    }

    pub fn repeat_count(&self) -> usize {
        self.view.repeat_count
    }

    pub fn set_repeat_count(&mut self, repeat_count: usize) {
        self.view.repeat_count = repeat_count.max(1);
        self.invalidate_cache();
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

    pub fn children(&self) -> Option<&[HistoryCell]> {
        match &self.content {
            HistoryContent::ToolGroup(cell) => Some(&cell.children),
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
            HistoryContent::ToolGroup(cell) => cell.summary = summary,
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
            HistoryContent::ToolGroup(cell) => &cell.label,
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
            HistoryKind::Tool => render_tool(self, width),
            HistoryKind::Notice => render_notice(self, width),
        }
    }

    pub fn to_transcript_lines(&self, width: usize) -> Vec<Line<'static>> {
        match self.kind() {
            HistoryKind::Message if self.tone == HistoryTone::User => render_user(self, width),
            HistoryKind::Message => render_agent_transcript(self, width),
            HistoryKind::Reasoning => render_reasoning(self, width),
            HistoryKind::Exploration => render_compact_transcript(self, width, "◦"),
            HistoryKind::Command => render_compact_transcript(self, width, "›"),
            HistoryKind::Tool => render_compact_transcript(self, width, "•"),
            HistoryKind::Notice => render_notice_transcript(self, width),
        }
    }

    pub fn to_live_transcript_lines(&self, width: usize) -> Vec<Line<'static>> {
        match self.kind() {
            HistoryKind::Reasoning => render_reasoning_live(self, width),
            _ => self.to_transcript_lines(width),
        }
    }

    pub fn rendered_line_count(lines: &[Line<'static>], width: usize) -> usize {
        if lines.is_empty() {
            return 0;
        }
        Paragraph::new(Text::from(lines.to_vec()))
            .wrap(Wrap { trim: false })
            .line_count(width as u16)
    }
}

impl PartialEq for HistoryCell {
    fn eq(&self, other: &Self) -> bool {
        self.tone == other.tone && self.content == other.content && self.view == other.view
    }
}

impl Eq for HistoryCell {}

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
                self.push(cell);
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
                    self.push(cell);
                }
            }
        }
    }

    pub fn push(&mut self, cell: HistoryCell) {
        self.cells.push(cell);
    }

    pub fn replace_cells(&mut self, cells: Vec<HistoryCell>) {
        self.cells.clear();
        for cell in cells {
            self.push(cell);
        }
    }

    pub fn set_tool_cells_expanded(&mut self, expanded: bool) {
        for cell in &mut self.cells {
            if matches!(
                cell.tone,
                HistoryTone::Reasoning
                    | HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.set_expanded(expanded);
            }
        }
    }

    pub fn cells(&self) -> &[HistoryCell] {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut Vec<HistoryCell> {
        &mut self.cells
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
    let inner = width.saturating_sub(2).max(8);
    word_wrap_text(cell.body(), WrapOptions::new(inner))
        .into_iter()
        .enumerate()
        .map(|(line_index, line)| {
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(if line_index == 0 {
                Span::styled("› ", Style::default().fg(Color::Rgb(140, 150, 170)))
            } else {
                Span::raw("  ")
            });
            spans.extend(
                line.spans
                    .into_iter()
                    .map(|span| {
                        Span::styled(
                            span.content.into_owned(),
                            Style::default()
                                .fg(Color::Rgb(220, 220, 235))
                                .add_modifier(Modifier::BOLD),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            Line::from(spans)
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
        Span::styled("≈ ", Style::default().fg(Color::Rgb(170, 140, 255))),
        Span::styled(
            if cell.label().is_empty() {
                "Reasoning".to_string()
            } else {
                cell.label().to_string()
            },
            Style::default()
                .fg(Color::Rgb(210, 215, 225))
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let subsequent_indent = Line::from(vec![
        Span::raw("    "),
        Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
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
            // Preserve paragraph spacing while keeping the same continuous reasoning gutter.
            out.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
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
        Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
        Span::styled(
            format!("… +{} lines", hidden_lines),
            Style::default().fg(Color::Rgb(132, 138, 150)),
        ),
    ]));
    kept
}

fn render_reasoning_summary(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    render_reasoning_lines(
        cell.body(),
        width,
        Style::default()
            .fg(Color::Rgb(148, 152, 164))
            .add_modifier(Modifier::ITALIC),
    )
}

fn render_reasoning_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let paragraphs = reasoning_paragraphs(text);
    let mut lines = Vec::new();

    for (index, paragraph) in paragraphs.into_iter().enumerate() {
        let initial_indent = if index == 0 {
            Line::from(vec![Span::styled(
                "• ",
                Style::default().fg(Color::Rgb(170, 140, 255)),
            )])
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
    let max_details = if cell.is_expanded() { 8 } else { 2 };
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("◦ ", Style::default().fg(Color::Rgb(120, 170, 255))),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(215, 220, 232))
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                ]))
                .subsequent_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                ])),
        )
        .into_iter()
        .map(tint_tail_rgb(190, 200, 216)),
    );

    for (index, detail) in details.iter().take(max_details).enumerate() {
        let indent = if index == 0 { "    └ " } else { "      " };
        lines.extend(
            word_wrap_text(
                detail,
                WrapOptions::new(width)
                    .initial_indent(Line::from(indent))
                    .subsequent_indent(Line::from("    ")),
            )
            .into_iter()
            .map(tint_all_rgb(190, 200, 216)),
        );
    }

    if details.len() > max_details {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                format!("… +{} more", details.len().saturating_sub(max_details)),
                Style::default().fg(Color::Rgb(148, 152, 164)),
            ),
        ]));
    }

    lines
}

fn render_command(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        "Command".to_string()
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("› ", Style::default().fg(Color::Rgb(120, 170, 255))),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(215, 220, 232))
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from("    "))
                .subsequent_indent(Line::from("    ")),
        )
        .into_iter()
        .map(tint_all_rgb(190, 200, 216)),
    );

    if let Some(detail) = cell.detail() {
        let raw_lines = detail
            .lines()
            .flat_map(|line| {
                word_wrap_text(
                    line,
                    WrapOptions::new(width)
                        .initial_indent(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("↳ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                        ]))
                        .subsequent_indent(Line::from("      ")),
                )
            })
            .collect::<Vec<_>>();
        let max_lines = if cell.is_expanded() { 24usize } else { 5usize };
        let display_lines: Vec<Line<'static>> = if raw_lines.len() <= max_lines {
            raw_lines
        } else {
            let mut kept = raw_lines.into_iter().take(max_lines).collect::<Vec<_>>();
            kept.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("↳ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                Span::styled(
                    format!(
                        "… +{} lines",
                        detail.lines().count().saturating_sub(max_lines)
                    ),
                    Style::default().fg(Color::Rgb(132, 138, 150)),
                ),
            ]));
            kept
        };
        lines.extend(display_lines.into_iter().map(tint_tail_rgb(132, 138, 150)));
    }

    lines
}

fn render_tool(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match &cell.content {
        HistoryContent::ToolGroup(group) => render_tool_group(cell, group, width),
        _ => render_tool_like(cell, width, Color::Rgb(120, 170, 255), "•"),
    }
}

fn render_tool_group(
    cell: &HistoryCell,
    group: &ToolGroupCell,
    width: usize,
) -> Vec<Line<'static>> {
    let title = pretty_tool_title(&group.label);
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled("• ", Style::default().fg(Color::Rgb(120, 170, 255))),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(210, 215, 225))
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    if !is_generic_tool_group_summary(&group.summary) {
        lines.extend(
            word_wrap_text(
                &group.summary,
                WrapOptions::new(width)
                    .initial_indent(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                    ]))
                    .subsequent_indent(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                    ])),
            )
            .into_iter()
            .map(tint_tail_rgb(148, 152, 164)),
        );
    }

    if !cell.is_expanded() {
        let preview_count = group.children.len().min(2);
        for (index, child) in group.children.iter().take(preview_count).enumerate() {
            let step_title = if child.label().is_empty() {
                "Step".to_string()
            } else {
                child.label().to_string()
            };
            let preview_body = compact_inline_preview(child.body(), 72);
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                Span::styled(
                    if index + 1 == preview_count && group.children.len() == 1 {
                        "└ "
                    } else {
                        "├ "
                    },
                    Style::default().fg(Color::Rgb(90, 96, 108)),
                ),
                Span::styled(
                    step_title,
                    Style::default()
                        .fg(Color::Rgb(215, 220, 232))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(preview_body, Style::default().fg(Color::Rgb(190, 200, 216))),
            ]));
        }
        let hidden_count = group.children.len().saturating_sub(preview_count);
        if hidden_count > 0 {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                Span::styled(
                    format!(
                        "{} more step{}",
                        hidden_count,
                        if hidden_count == 1 { "" } else { "s" }
                    ),
                    Style::default().fg(Color::Rgb(148, 152, 164)),
                ),
            ]));
        }
        return lines;
    }

    for (index, child) in group.children.iter().enumerate() {
        let is_last = index + 1 == group.children.len();
        lines.extend(render_tool_group_child(child, width, is_last));
    }

    lines
}

fn render_tool_group_child(cell: &HistoryCell, width: usize, is_last: bool) -> Vec<Line<'static>> {
    let branch = if is_last { "└ " } else { "├ " };
    let rail = if is_last { "  " } else { "│ " };
    let title = if cell.label().is_empty() {
        "Step".to_string()
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::raw("    "),
        Span::styled(branch, Style::default().fg(Color::Rgb(90, 96, 108))),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(215, 220, 232))
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(rail, Style::default().fg(Color::Rgb(90, 96, 108))),
                ]))
                .subsequent_indent(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(rail, Style::default().fg(Color::Rgb(90, 96, 108))),
                ])),
        )
        .into_iter()
        .map(tint_tail_rgb(190, 200, 216)),
    );

    if let Some(detail) = cell.detail() {
        let raw_lines = detail
            .lines()
            .flat_map(|line| {
                word_wrap_text(
                    line,
                    WrapOptions::new(width)
                        .initial_indent(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(rail, Style::default().fg(Color::Rgb(90, 96, 108))),
                            Span::styled("↳ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                        ]))
                        .subsequent_indent(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(rail, Style::default().fg(Color::Rgb(90, 96, 108))),
                            Span::raw("  "),
                        ])),
                )
            })
            .collect::<Vec<_>>();
        let max_lines = if cell.is_expanded() { 12usize } else { 3usize };
        let display_lines: Vec<Line<'static>> = if raw_lines.len() <= max_lines {
            raw_lines
        } else {
            let mut kept = raw_lines.into_iter().take(max_lines).collect::<Vec<_>>();
            kept.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(rail, Style::default().fg(Color::Rgb(90, 96, 108))),
                Span::styled(
                    format!(
                        "… +{} more lines",
                        detail.lines().count().saturating_sub(max_lines)
                    ),
                    Style::default().fg(Color::Rgb(132, 138, 150)),
                ),
            ]));
            kept
        };
        lines.extend(display_lines.into_iter().map(tint_tail_rgb(132, 138, 150)));
    }

    lines
}

fn render_tool_like(
    cell: &HistoryCell,
    width: usize,
    accent: Color,
    dot: &str,
) -> Vec<Line<'static>> {
    let title = pretty_tool_title(cell.label());
    let title = if cell.repeat_count() > 1 {
        format!("{title} x{}", cell.repeat_count())
    } else {
        title
    };
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{dot} "), Style::default().fg(accent)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(210, 215, 225))
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    let wrapped = word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
            ]))
            .subsequent_indent(Line::from(vec![
                Span::raw("    "),
                Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
            ])),
    );
    let max_lines = if cell.is_expanded() { 24usize } else { 2usize };
    let mut output_lines: Vec<Line<'static>> = Vec::new();
    if wrapped.len() <= max_lines {
        output_lines.extend(wrapped);
    } else {
        output_lines.extend(wrapped.iter().take(max_lines).cloned());
        output_lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("│ ", Style::default().fg(Color::Rgb(90, 96, 108))),
            Span::styled(
                format!("… +{} lines", wrapped.len().saturating_sub(max_lines)),
                Style::default().fg(Color::Rgb(148, 152, 164)),
            ),
        ]));
    }
    lines.extend(
        output_lines
            .into_iter()
            .filter(|line| !line.spans.is_empty())
            .map(tint_tail_rgb(148, 152, 164)),
    );

    if let Some(detail) = cell.detail() {
        let raw_lines = detail
            .lines()
            .flat_map(|line| {
                word_wrap_text(
                    line,
                    WrapOptions::new(width)
                        .initial_indent(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("└ ", Style::default().fg(Color::Rgb(90, 96, 108))),
                        ]))
                        .subsequent_indent(Line::from(vec![
                            Span::raw("      "),
                            Span::styled("  ", Style::default().fg(Color::Rgb(90, 96, 108))),
                        ])),
                )
            })
            .collect::<Vec<_>>();
        let max_detail_lines = if cell.is_expanded() { 12usize } else { 3usize };
        let display_lines: Vec<Line<'static>> = if raw_lines.len() <= max_detail_lines {
            raw_lines
        } else {
            let mut kept = raw_lines
                .into_iter()
                .take(max_detail_lines)
                .collect::<Vec<_>>();
            kept.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(
                    format!(
                        "… +{} more lines",
                        detail.lines().count().saturating_sub(max_detail_lines)
                    ),
                    Style::default().fg(Color::Rgb(132, 138, 150)),
                ),
            ]));
            kept
        };
        lines.extend(display_lines.into_iter().map(tint_tail_rgb(132, 138, 150)));
    }
    lines
}

fn render_meta(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from(vec![Span::styled(
                "· ",
                Style::default().fg(Color::Rgb(80, 80, 90)),
            )]))
            .subsequent_indent(Line::from("  ")),
    )
    .into_iter()
    .map(tint_tail_rgb(110, 110, 120))
    .collect()
}

fn render_notice(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    match cell.tone {
        HistoryTone::Warning => render_tool_like(cell, width, Color::Rgb(255, 180, 50), "◆"),
        HistoryTone::Error => render_tool_like(cell, width, Color::Rgb(255, 80, 80), "◆"),
        HistoryTone::Meta => render_meta(cell, width),
        _ => render_tool_like(cell, width, Color::Rgb(120, 170, 255), "•"),
    }
}

fn render_notice_transcript(cell: &HistoryCell, width: usize) -> Vec<Line<'static>> {
    let accent = match cell.tone {
        HistoryTone::Error => Color::Rgb(255, 120, 120),
        HistoryTone::Warning => Color::Rgb(255, 196, 108),
        HistoryTone::Control => Color::Rgb(132, 138, 150),
        _ => Color::Rgb(132, 138, 150),
    };
    let mut title = cell.label().to_string();
    if title.is_empty() {
        title = "Notice".to_string();
    }
    let body_lines = word_wrap_text(
        cell.body(),
        WrapOptions::new(width)
            .initial_indent(Line::from("  "))
            .subsequent_indent(Line::from("  ")),
    )
    .into_iter()
    .map(tint_tail_rgb(190, 200, 216))
    .collect::<Vec<_>>();

    let mut lines = vec![Line::from(vec![
        Span::styled("• ", Style::default().fg(accent)),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(210, 215, 225))
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    lines.extend(body_lines);
    lines
}

fn render_compact_transcript(cell: &HistoryCell, width: usize, bullet: &str) -> Vec<Line<'static>> {
    let title = if cell.label().is_empty() {
        match cell.kind() {
            HistoryKind::Exploration => "Explored workspace".to_string(),
            HistoryKind::Command => "Command".to_string(),
            HistoryKind::Tool => "Tool".to_string(),
            _ => "Step".to_string(),
        }
    } else {
        cell.label().to_string()
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{bullet} "),
            Style::default().fg(Color::Rgb(120, 170, 255)),
        ),
        Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(210, 215, 225))
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    lines.extend(
        word_wrap_text(
            cell.body(),
            WrapOptions::new(width)
                .initial_indent(Line::from("  "))
                .subsequent_indent(Line::from("  ")),
        )
        .into_iter()
        .map(tint_tail_rgb(190, 200, 216)),
    );
    lines
}

fn pretty_tool_title(label: &str) -> String {
    match label {
        "context" => "Context".to_string(),
        "conversation" => "conversation".to_string(),
        "reasoning" => "reasoning".to_string(),
        other => humanize_tool_label(other),
    }
}

fn compact_inline_preview(input: &str, max_chars: usize) -> String {
    let trimmed = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn is_generic_tool_group_summary(summary: &str) -> bool {
    matches!(
        summary.trim().to_ascii_lowercase().as_str(),
        "exploring workspace" | "running tool"
    )
}

fn tint_all_rgb(r: u8, g: u8, b: u8) -> impl Fn(Line<'static>) -> Line<'static> {
    move |line| {
        let spans = line
            .spans
            .into_iter()
            .map(|span| {
                Span::styled(
                    span.content.into_owned(),
                    Style::default().fg(Color::Rgb(r, g, b)),
                )
            })
            .collect::<Vec<_>>();
        Line::from(spans)
    }
}

fn tint_tail_rgb(r: u8, g: u8, b: u8) -> impl Fn(Line<'static>) -> Line<'static> {
    move |line| {
        let spans = line
            .spans
            .into_iter()
            .enumerate()
            .map(|(index, span)| {
                if index == 0 {
                    span
                } else {
                    Span::styled(
                        span.content.into_owned(),
                        Style::default().fg(Color::Rgb(r, g, b)),
                    )
                }
            })
            .collect::<Vec<_>>();
        Line::from(spans)
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
    use super::{ExplorationAggregate, HistoryCell, HistoryFormat, HistoryTone, tool_aggregation};

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

    fn joined_lines(lines: Vec<ratatui::text::Line<'static>>) -> String {
        lines
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
    fn agent_cells_render_without_shell_bullet_prefix() {
        let cell = HistoryCell::agent(
            "cloudagent",
            "### 也就是说\n\n逻辑已经改对了。\n\n1. 查锁\n2. 重跑",
            HistoryFormat::Markdown,
        );

        let rendered = joined(&cell, 100);
        let transcript = joined_lines(cell.to_transcript_lines(100));

        assert!(rendered.starts_with("  ### 也就是说"));
        assert!(transcript.starts_with("  ### 也就是说"));
        assert!(!rendered.contains("●"));
        assert!(!rendered.contains("• ###"));
        assert!(!rendered.contains("• 逻辑"));
        assert!(!transcript.contains("• ###"));
        assert!(!transcript.contains("• 逻辑"));
    }

    #[test]
    fn agent_cells_keep_codex_style_left_padding_without_bullet() {
        let cell = HistoryCell::agent("cloudagent", "正文\n第二行", HistoryFormat::Markdown);

        let plain = cell
            .to_lines_with_mode(80)
            .into_iter()
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(plain, vec!["  正文", "  第二行"]);
    }

    #[test]
    fn plaintext_cells_do_not_get_markdown_table_rendering() {
        let cell = HistoryCell::agent("tool", "| raw | text |", HistoryFormat::PlainText);

        let rendered = joined(&cell, 100);
        assert!(rendered.contains("| raw | text |"));
    }

    #[test]
    fn reasoning_cells_wrap_without_terminal_hard_break_artifacts() {
        let cell = HistoryCell::reasoning(
            "reasoning",
            "Now let me look at the collect_repo_entries function to understand how it handles paths, and also check if there's any path validation that rejects relative paths.",
        );

        let rendered = joined(&cell, 80);
        assert!(!rendered.contains("pat\n    │ h"));
        assert!(rendered.contains("path"));
    }

    #[test]
    fn reasoning_multiline_paragraphs_keep_a_single_header() {
        let cell = HistoryCell::reasoning(
            "reasoning",
            "Now I have a clear picture.\n1. resolve_read_path allows absolute paths.\n2. resolve_full_access_path allows absolute paths.",
        );

        let rendered = joined(&cell, 100);
        assert_eq!(rendered.matches("≈ reasoning").count(), 1);
        assert!(rendered.contains("Now I have a clear picture."));
        assert!(rendered.contains("1. resolve_read_path"));
        assert!(rendered.contains("2."));
        assert!(rendered.contains("resolve_full_access_path"));
        assert!(!rendered.contains("\n\n"));
    }

    #[test]
    fn reasoning_single_newlines_collapse_into_compact_paragraphs() {
        let cell = HistoryCell::reasoning(
            "reasoning",
            "方案：只修改 exec_command 的 workdir 处理逻辑。\n但 resolve_read_path 允许绝对路径。\n所以需要评估权限边界。",
        );

        let rendered = joined(&cell, 120);
        assert_eq!(rendered.matches("≈ reasoning").count(), 1);
        assert!(!rendered.contains("\n\n"));
        assert!(rendered.contains("方案：只修改 exec_command 的 workdir 处理逻辑。"));
        assert!(rendered.contains("但 resolve_read_path 允许绝对路径。"));
    }

    #[test]
    fn user_cells_wrap_fully_without_intrinsic_truncation() {
        let cell = HistoryCell::user(
            "one two three four five six seven eight nine ten eleven twelve thirteen fourteen",
        );

        let rendered = joined(&cell, 14);
        assert!(rendered.contains("› one two"));
        assert!(rendered.contains("three four"));
        assert!(rendered.contains("thirteen"));
        assert!(!rendered.contains("... +"));
        assert!(!rendered.contains("… +"));
    }

    #[test]
    fn user_cells_only_prefix_first_multiline_row() {
        let cell = HistoryCell::user("first line\nsecond line\nthird line");

        let rendered = cell.to_lines_with_mode(80);
        let plain = rendered
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(plain, vec!["› first line", "  second line", "  third line"]);
    }

    #[test]
    fn exploration_cells_render_summary_with_nested_details() {
        let mut aggregate = ExplorationAggregate::new("file search `cli`".to_string());
        aggregate.searches = 8;
        aggregate.read_files = 10;
        aggregate.push_detail("text search `clipboard`".to_string());
        aggregate.push_detail("Read input_mapping.rs".to_string());
        aggregate.push_detail("Read textarea.rs".to_string());
        let cell = HistoryCell::exploration(
            "Explored workspace",
            "searched 8 times, read 10 files",
            aggregate,
            HistoryTone::Control,
        );

        let rendered = joined(&cell, 120);
        assert!(rendered.contains("Explored workspace"));
        assert!(rendered.contains("searched 8 times, read 10 files"));
        assert!(rendered.contains("└ file search `cli`"));
        assert!(rendered.contains("text search `clipboard`"));
    }

    #[test]
    fn transcript_merges_adjacent_agent_stream_continuations() {
        let mut first = HistoryCell::agent("", "hello", HistoryFormat::Markdown);
        let second = HistoryCell::agent("", " world", HistoryFormat::Markdown)
            .with_stream_continuation(true);

        assert!(tool_aggregation::coalesce_agent_stream(&mut first, &second));
        assert_eq!(first.body(), "hello world");
    }

    #[test]
    fn transcript_does_not_merge_agent_cells_across_non_agent_boundaries() {
        let first = HistoryCell::agent("", "hello", HistoryFormat::Markdown);
        let barrier = HistoryCell::reasoning("Reasoning", "thinking");
        let second = HistoryCell::agent("", " world", HistoryFormat::Markdown)
            .with_stream_continuation(true);

        let mut cells = Vec::new();
        for cell in [first, barrier, second] {
            if let Some(last) = cells.last_mut()
                && tool_aggregation::coalesce_agent_stream(last, &cell)
            {
                continue;
            }
            if let Some(last) = cells.last_mut()
                && tool_aggregation::coalesce_tool_like(last, &cell, true)
            {
                continue;
            }
            cells.push(cell);
        }

        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].body(), "hello");
        assert_eq!(cells[1].body(), "thinking");
        assert_eq!(cells[2].body(), " world");
    }
}
