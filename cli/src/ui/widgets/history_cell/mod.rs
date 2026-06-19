mod markdown;
mod display;
mod render;
mod transcript;
pub(crate) mod tool_aggregation;
mod tool_ui;
mod wrapping;

use ratatui::text::Line;

pub(crate) use render::{
    RenderContext, humanize_tool_label, render_active_item_placeholder, render_history_entry,
};
pub(crate) use transcript::Transcript;

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
            let lines = display::render_cell_lines(self, width);
            *cache = Some((width, lines.clone()));
            lines
        } else {
            display::render_cell_lines(self, width)
        }
    }

    pub fn to_transcript_lines(&self, width: usize) -> Vec<Line<'static>> {
        display::render_transcript_lines(self, width)
    }

    pub fn to_live_transcript_lines(&self, width: usize) -> Vec<Line<'static>> {
        display::render_live_transcript_lines(self, width)
    }

    pub fn rendered_line_count(lines: &[Line<'static>], width: usize) -> usize {
        display::rendered_line_count(lines, width)
    }
}

impl PartialEq for HistoryCell {
    fn eq(&self, other: &Self) -> bool {
        self.tone == other.tone && self.content == other.content && self.view == other.view
    }
}

impl Eq for HistoryCell {}

fn default_kind_for_tone(tone: HistoryTone) -> HistoryKind {
    match tone {
        HistoryTone::User | HistoryTone::Agent => HistoryKind::Message,
        HistoryTone::Reasoning => HistoryKind::Reasoning,
        HistoryTone::Tool | HistoryTone::Control => HistoryKind::Tool,
        HistoryTone::Warning | HistoryTone::Error | HistoryTone::Meta => HistoryKind::Notice,
    }
}

#[cfg(test)]
mod tests;
