pub mod reducer;
pub mod runtime_projection;
pub mod selectors;
pub mod status_view_model;

use crate::ui::widgets::history_cell::{ExplorationAggregate, HistoryCell, HistoryTone, Transcript};
use agent_protocol::{ConversationTurn, FrontendMode, ModelUsage, RequestId};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ConsoleState {
    pub mode: FrontendMode,
}

impl ConsoleState {
    pub fn new() -> Self {
        Self {
            mode: FrontendMode::Idle,
        }
    }

    pub fn can_submit_turn(&self) -> bool {
        self.mode == FrontendMode::Idle
    }
}

impl Default for ConsoleState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ServerRequestState {
    pub active_request_id: Option<RequestId>,
    pub action_required: bool,
}

#[derive(Default)]
pub struct TranscriptState {
    pub transcript: Transcript,
    pub active_assistant: Option<ActiveAssistantState>,
    pub active_exec_view: Option<ActiveExecViewState>,
    pub active_exec: Option<ActiveExecSession>,
    pub active_reasoning: Option<ActiveReasoningState>,
    pub next_overlay_order: u64,
    pub live_overlays: Vec<LiveOverlayEntry>,
    pub next_live_block_id: u64,
    pub last_copyable_output: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ActiveAssistantState {
    pub block_id: u64,
    pub item_id: String,
    pub cell: HistoryCell,
    pub order: u64,
    pub completed: bool,
}

#[derive(Clone, Debug)]
pub struct ActiveReasoningState {
    pub block_id: u64,
    pub item_id: String,
    pub title: String,
    pub text: String,
    pub order: u64,
    pub completed: bool,
}

#[derive(Clone, Debug)]
pub struct ActiveExecViewState {
    pub block_id: u64,
    pub presentation: ActiveExecPresentation,
    pub order: u64,
    pub committed: bool,
}

#[derive(Clone, Debug)]
pub enum ActiveExecPresentation {
    Command {
        label: String,
        summary: String,
        detail: Option<String>,
        expanded: bool,
    },
    Exploration {
        label: String,
        summary: String,
        aggregate: ExplorationAggregate,
        expanded: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveOverlayKind {
    Assistant,
    Exec,
    Reasoning,
}

#[derive(Clone, Debug)]
pub struct LiveOverlayEntry {
    pub id: u64,
    pub kind: LiveOverlayKind,
    pub cell: HistoryCell,
}

#[derive(Clone, Debug)]
pub struct ActiveExecSession {
    pub mode: ActiveExecMode,
    pub calls: Vec<ActiveExecCall>,
}

#[derive(Clone, Debug)]
pub enum ActiveExecMode {
    Exploration { aggregate: ExplorationAggregate },
    Command,
}

#[derive(Clone, Debug)]
pub struct ActiveExecCall {
    pub route_key: ActiveExecRouteKey,
    pub label: String,
    pub summary: String,
    pub detail: String,
    pub completed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActiveExecRouteKey {
    CallId(String),
    ItemId(String),
}

impl ActiveAssistantState {
    pub fn to_history_cell(&self) -> HistoryCell {
        self.cell.clone()
    }
}

impl ActiveReasoningState {
    pub fn to_history_cell(&self) -> HistoryCell {
        let mut cell = HistoryCell::reasoning(self.title.clone(), compact_live_reasoning_text(&self.text));
        cell.expanded = !self.completed;
        cell
    }
}

impl ActiveExecViewState {
    pub fn to_history_cell(&self) -> HistoryCell {
        self.presentation.to_history_cell()
    }
}

impl ActiveExecPresentation {
    pub fn to_history_cell(&self) -> HistoryCell {
        match self {
            ActiveExecPresentation::Command {
                label,
                summary,
                detail,
                expanded,
            } => {
                let mut cell = HistoryCell::exec(
                    label.clone(),
                    summary.clone(),
                    detail.clone(),
                    HistoryTone::Control,
                );
                cell.expanded = *expanded;
                cell
            }
            ActiveExecPresentation::Exploration {
                label,
                summary,
                aggregate,
                expanded,
            } => {
                let mut cell = HistoryCell::exploration(
                    label.clone(),
                    summary.clone(),
                    aggregate.clone(),
                    HistoryTone::Control,
                );
                cell.expanded = *expanded;
                cell
            }
        }
    }
}

impl ActiveExecSession {
    pub fn new_command(call: ActiveExecCall) -> Self {
        Self {
            mode: ActiveExecMode::Command,
            calls: vec![call],
        }
    }

    pub fn new_exploration(call: ActiveExecCall) -> Self {
        Self {
            mode: ActiveExecMode::Exploration {
                aggregate: ExplorationAggregate::new(String::new()),
            },
            calls: vec![call],
        }
    }

    pub fn is_exploration(&self) -> bool {
        matches!(self.mode, ActiveExecMode::Exploration { .. })
    }

    pub fn has_pending_calls(&self) -> bool {
        self.calls.iter().any(|call| !call.completed)
    }

    pub fn append_call(&mut self, call: ActiveExecCall) {
        self.calls.push(call);
    }

    pub fn append_delta(&mut self, route_key: &ActiveExecRouteKey, delta: &str) -> bool {
        let is_exploration = self.is_exploration();
        let Some(call) = self
            .calls
            .iter_mut()
            .rev()
            .find(|call| &call.route_key == route_key)
        else {
            return false;
        };
        let chunk = if is_exploration {
            delta.trim()
        } else {
            delta
        };
        if chunk.is_empty() {
            return true;
        }
        if is_exploration && !call.detail.trim().is_empty() {
            call.detail.push_str(" — ");
        }
        call.detail.push_str(chunk);
        true
    }

    pub fn complete_call(&mut self, route_key: &ActiveExecRouteKey) -> bool {
        let Some(call) = self
            .calls
            .iter_mut()
            .rev()
            .find(|call| &call.route_key == route_key)
        else {
            return false;
        };
        call.completed = true;
        true
    }

    pub fn complete_call_or_only_pending(&mut self, route_key: &ActiveExecRouteKey) -> bool {
        if self.complete_call(route_key) {
            return true;
        }
        let mut pending = self
            .calls
            .iter_mut()
            .enumerate()
            .filter(|(_, call)| !call.completed);
        let Some((first_index, _)) = pending.next() else {
            return false;
        };
        if pending.next().is_some() {
            return false;
        }
        self.calls[first_index].completed = true;
        true
    }

    pub fn contains_call(&self, route_key: &ActiveExecRouteKey) -> bool {
        self.calls.iter().any(|call| &call.route_key == route_key)
    }

    pub fn exploration_aggregate_mut(&mut self) -> Option<&mut ExplorationAggregate> {
        match &mut self.mode {
            ActiveExecMode::Exploration { aggregate } => Some(aggregate),
            ActiveExecMode::Command => None,
        }
    }

    pub fn exploration_aggregate(&self) -> Option<&ExplorationAggregate> {
        match &self.mode {
            ActiveExecMode::Exploration { aggregate } => Some(aggregate),
            ActiveExecMode::Command => None,
        }
    }

    pub fn last_call(&self) -> Option<&ActiveExecCall> {
        self.calls.last()
    }
}

#[derive(Clone, Debug)]
pub struct SystemNotice {
    pub text: String,
    pub expires_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct RunState {
    pub history_loaded: bool,
    pub history_snapshot: Option<Vec<ConversationTurn>>,
    pub system_notice: Option<SystemNotice>,
    pub last_turn_usage: Option<ModelUsage>,
    pub total_turn_usage: Option<ModelUsage>,
    pub model_context_window: Option<u64>,
    pub should_exit: bool,
    pub live_animation_frame: u64,
    pub expand_tool_details: bool,
    pub pre_llm_filter_enabled: bool,
    pub permission_mode: String,
}

impl RunState {
    pub fn new(connection_label: &str) -> Self {
        Self {
            history_loaded: false,
            history_snapshot: None,
            system_notice: Some(SystemNotice {
                text: format!("Connected via {connection_label}"),
                expires_at: None,
            }),
            last_turn_usage: None,
            total_turn_usage: None,
            model_context_window: None,
            should_exit: false,
            live_animation_frame: 0,
            expand_tool_details: false,
            pre_llm_filter_enabled: false,
            permission_mode: "ReadOnly".to_string(),
        }
    }
}

impl RunState {
    fn default_ttl(level: NoticeLevel) -> Duration {
        match level {
            NoticeLevel::Info => Duration::from_secs(3),
            NoticeLevel::Warn => Duration::from_secs(5),
            NoticeLevel::Error => Duration::from_secs(8),
        }
    }

    pub fn set_system_notice(&mut self, text: impl Into<String>, ttl: Option<Duration>) {
        self.system_notice = Some(SystemNotice {
            text: text.into(),
            expires_at: ttl.map(|d| Instant::now() + d),
        });
    }

    pub fn set_system_notice_level(&mut self, text: impl Into<String>, level: NoticeLevel) {
        self.set_system_notice(text, Some(Self::default_ttl(level)));
    }

    pub fn clear_system_notice(&mut self) {
        self.system_notice = None;
    }

    pub fn current_system_notice(&self) -> Option<&str> {
        self.system_notice
            .as_ref()
            .and_then(|n| match n.expires_at {
                Some(t) if Instant::now() > t => None,
                _ => Some(n.text.as_str()),
            })
    }

    pub fn clear_expired_notices(&mut self) {
        if self
            .system_notice
            .as_ref()
            .and_then(|n| n.expires_at)
            .is_some_and(|t| Instant::now() > t)
        {
            self.system_notice = None;
        }
    }
}

fn compact_live_reasoning_text(text: &str) -> String {
    let mut paragraphs = Vec::new();
    let mut current = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.starts_with("```") {
            continue;
        }
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
            continue;
        }
        let compact = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !compact.is_empty() {
            current.push(compact);
        }
    }

    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }

    if paragraphs.is_empty() {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        paragraphs.join("\n\n")
    }
}
