use super::streaming::{AgentStreamController, AgentStreamFinish, AgentStreamOutput};
use crate::ui::history_cell::{
    HistoryCell, humanize_tool_label, render_active_item_placeholder, render_history_entry,
};
use agent_core::conversation::{InputItem, TranscriptItem, input_items_to_plain_text};
use agent_core::is_web_search_tool_result;
use agent_core::turn::{TurnId, TurnItemKind};
use std::collections::HashSet;

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ActiveTurnAction {
    Clear,
    StartLocalUser {
        user_input: Vec<InputItem>,
    },
    BindTurnId {
        turn_id: TurnId,
    },
    StartItem {
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
    },
    AppendAgentDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendReasoningDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    AppendToolDelta {
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    CompleteItem {
        turn_id: TurnId,
        item_id: String,
        item: TranscriptItem,
    },
    CompleteTurn {
        turn_id: TurnId,
    },
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ActiveTurnEffects {
    pub(crate) active_cell: Option<HistoryCell>,
    pub(crate) last_copyable_output: Option<String>,
    pub(crate) replay_cells: Vec<HistoryCell>,
    pub(crate) consolidate_agent_message: Option<ConsolidateAgentMessage>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConsolidateAgentMessage {
    pub(crate) item_id: String,
    pub(crate) cell: HistoryCell,
}

#[derive(Default)]
pub(crate) struct ActiveTurnState {
    turn_id: Option<TurnId>,
    live_item: Option<ActiveItemView>,
    last_copyable_output: Option<String>,
    replayed_item_ids: HashSet<String>,
    pending_local_user_input: Option<Vec<InputItem>>,
    agent_stream: Option<AgentStreamController>,
}

#[derive(Clone)]
struct ActiveItemView {
    item_id: String,
    kind: TurnItemKind,
    cell: HistoryCell,
}

impl ActiveItemView {
    fn new(item_id: impl Into<String>, kind: TurnItemKind, cell: HistoryCell) -> Self {
        Self {
            item_id: item_id.into(),
            kind,
            cell,
        }
    }

    fn into_cell(self) -> HistoryCell {
        self.cell
    }

    fn to_cell(&self) -> HistoryCell {
        self.cell.clone()
    }

    fn append_body(&mut self, delta: &str) {
        self.cell.append_body(delta);
    }

    fn replace_body(&mut self, body: impl Into<String>) {
        self.cell.replace_body(body);
    }

    fn body(&self) -> &str {
        self.cell.body()
    }

    fn label(&self) -> &str {
        self.cell.label()
    }
}

impl ActiveTurnState {
    pub(crate) fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    pub(crate) fn clear(&mut self) -> ActiveTurnEffects {
        self.turn_id = None;
        self.live_item = None;
        self.last_copyable_output = None;
        self.replayed_item_ids.clear();
        self.pending_local_user_input = None;
        self.agent_stream = None;
        ActiveTurnEffects::default()
    }

    pub(crate) fn apply(&mut self, action: ActiveTurnAction) -> ActiveTurnEffects {
        match action {
            ActiveTurnAction::Clear => self.clear(),
            ActiveTurnAction::StartLocalUser { user_input } => {
                self.turn_id = None;
                self.live_item = None;
                self.last_copyable_output = None;
                self.replayed_item_ids.clear();
                self.pending_local_user_input = Some(user_input);
                self.agent_stream = None;
                ActiveTurnEffects {
                    active_cell: None,
                    last_copyable_output: None,
                    replay_cells: vec![HistoryCell::user(input_items_to_plain_text(
                        self.pending_local_user_input.as_deref().unwrap_or_default(),
                    ))],
                    consolidate_agent_message: None,
                }
            }
            ActiveTurnAction::BindTurnId { turn_id } => {
                self.ensure_turn(&turn_id);
                self.snapshot_effects()
            }
            ActiveTurnAction::StartItem {
                turn_id,
                item_id,
                kind,
                title,
            } => {
                self.ensure_turn(&turn_id);
                let replay_cells = self.flush_live_tail_if_different(&item_id);
                if matches!(kind, TurnItemKind::CommandExecution) {
                    return self.snapshot_effects_with_replay(replay_cells);
                }
                self.live_item = Some(ActiveItemView::new(
                    item_id,
                    kind.clone(),
                    render_active_item_placeholder(kind, title.as_deref().unwrap_or("")),
                ));
                self.snapshot_effects_with_replay(replay_cells)
            }
            ActiveTurnAction::AppendAgentDelta {
                turn_id,
                item_id,
                delta,
            } => {
                self.ensure_turn(&turn_id);
                let mut replay_cells = self.ensure_agent_stream_tail(&item_id);
                let output = self.push_agent_stream_delta(&item_id, &delta);
                replay_cells.extend(output.stable_cells);
                self.last_copyable_output = Some(format!(
                    "{}{}",
                    self.last_copyable_output.as_deref().unwrap_or(""),
                    delta
                ));
                self.snapshot_effects_with_replay(replay_cells)
            }
            ActiveTurnAction::AppendReasoningDelta {
                turn_id,
                item_id,
                delta,
            } => {
                self.ensure_turn(&turn_id);
                let replay_cells = self
                    .ensure_live_tail(&item_id, HistoryCell::reasoning("Reasoning", "thinking"));
                if let Some(live_item) = self.live_item.as_mut() {
                    if live_item.body() == "thinking" {
                        live_item.replace_body(delta.clone());
                    } else {
                        live_item.append_body(&delta);
                    }
                }
                self.snapshot_effects_with_replay(replay_cells)
            }
            ActiveTurnAction::AppendToolDelta {
                turn_id,
                item_id,
                delta,
            } => {
                self.ensure_turn(&turn_id);
                let replay_cells = self.flush_live_tail_if_different(&item_id);
                if let Some(live_item) = self.live_item.as_mut()
                    && live_item.item_id == item_id
                {
                    match live_item.body().trim() {
                        "" | "running" => live_item.replace_body(delta),
                        _ => live_item.append_body(&delta),
                    }
                }
                self.snapshot_effects_with_replay(replay_cells)
            }
            ActiveTurnAction::CompleteItem {
                turn_id,
                item_id,
                item,
            } => {
                self.ensure_turn(&turn_id);
                let mut replay_cells = Vec::new();
                let cell = render_history_entry(&item, &mut Default::default());
                if let Some(text) = copyable_output(&item) {
                    self.last_copyable_output = Some(text);
                }
                if matches!(item, TranscriptItem::AgentMessage { .. }) {
                    let streamed =
                        self.take_finished_agent_stream(&item_id, completed_agent_text(&item));
                    let should_consolidate_stream = streamed
                        .as_ref()
                        .is_some_and(|streamed| streamed.emitted_any)
                        || self.replayed_item_ids.contains(&item_id);
                    if self
                        .live_item
                        .as_ref()
                        .is_some_and(|live_item| live_item.item_id == item_id)
                    {
                        self.live_item = None;
                    }
                    if !cell.is_empty() {
                        self.replayed_item_ids.insert(item_id.clone());
                        let mut effects = self.snapshot_effects_with_replay(replay_cells);
                        if should_consolidate_stream {
                            effects.consolidate_agent_message =
                                Some(ConsolidateAgentMessage { item_id, cell });
                        } else {
                            effects.replay_cells.push(cell);
                        }
                        return effects;
                    }
                }
                if self
                    .live_item
                    .as_ref()
                    .is_some_and(|live_item| live_item.item_id == item_id)
                    || self.should_replace_live_tool_placeholder(&item)
                {
                    if matches!(item, TranscriptItem::AgentMessage { .. })
                        && let Some(streamed) =
                            self.take_finished_agent_stream(&item_id, completed_agent_text(&item))
                    {
                        if streamed.emitted_any {
                            replay_cells.extend(streamed.stable_cells);
                        } else if !cell.is_empty() {
                            replay_cells.push(cell);
                        }
                    } else if !cell.is_empty() && should_keep_completed_item_live(&item) {
                        self.live_item =
                            Some(ActiveItemView::new(item_id, turn_item_kind(&item), cell));
                        return self.snapshot_effects_with_replay(replay_cells);
                    } else if !cell.is_empty() {
                        replay_cells.push(cell);
                    }
                    self.live_item = None;
                } else if !self.replayed_item_ids.contains(&item_id) && !cell.is_empty() {
                    self.replayed_item_ids.insert(item_id.clone());
                    replay_cells.push(cell);
                }
                self.snapshot_effects_with_replay(replay_cells)
            }
            ActiveTurnAction::CompleteTurn { turn_id } => {
                if self.turn_id.as_deref() != Some(turn_id.as_str()) {
                    return self.clear();
                }
                let replay_cells = if self.should_discard_live_cell_on_flush() {
                    self.live_item.take();
                    Vec::new()
                } else {
                    self.live_item
                        .take()
                        .map(ActiveItemView::into_cell)
                        .into_iter()
                        .collect::<Vec<_>>()
                };
                let last_copyable_output = self.last_copyable_output.clone();
                self.turn_id = None;
                self.last_copyable_output = None;
                self.replayed_item_ids.clear();
                self.pending_local_user_input = None;
                self.agent_stream = None;
                ActiveTurnEffects {
                    active_cell: None,
                    last_copyable_output,
                    replay_cells,
                    consolidate_agent_message: None,
                }
            }
        }
    }

    fn ensure_turn(&mut self, turn_id: &str) {
        match self.turn_id.as_deref() {
            Some(existing) if existing == turn_id => {}
            Some(_) => {
                self.turn_id = Some(turn_id.to_string());
                self.live_item = None;
                self.last_copyable_output = None;
                self.replayed_item_ids.clear();
                self.pending_local_user_input = None;
                self.agent_stream = None;
            }
            None => {
                self.turn_id = Some(turn_id.to_string());
            }
        }
    }

    fn ensure_live_tail(&mut self, item_id: &str, placeholder: HistoryCell) -> Vec<HistoryCell> {
        let replay_cells = self.flush_live_tail_if_different(item_id);
        if self
            .live_item
            .as_ref()
            .is_none_or(|live_item| live_item.item_id != item_id)
        {
            self.live_item = Some(ActiveItemView::new(
                item_id,
                TurnItemKind::ToolResult,
                placeholder,
            ));
        }
        replay_cells
    }

    fn ensure_agent_stream_tail(&mut self, item_id: &str) -> Vec<HistoryCell> {
        let replay_cells = self.flush_live_tail_if_different(item_id);
        if self
            .agent_stream
            .as_ref()
            .map(AgentStreamController::item_id)
            != Some(item_id)
        {
            self.agent_stream = Some(AgentStreamController::new(item_id));
        }
        if self.live_item.as_ref().is_some_and(|item| {
            item.item_id == item_id && item.kind == TurnItemKind::AssistantMessage
        }) || self.live_item.as_ref().map(|item| item.item_id.as_str()) != Some(item_id)
        {
            self.live_item = None;
        }
        replay_cells
    }

    fn push_agent_stream_delta(&mut self, item_id: &str, delta: &str) -> AgentStreamOutput {
        let Some(stream) = self.agent_stream.as_mut() else {
            return AgentStreamOutput {
                stable_cells: Vec::new(),
                live_cell: None,
            };
        };
        if stream.item_id() != item_id {
            return AgentStreamOutput {
                stable_cells: Vec::new(),
                live_cell: None,
            };
        }
        stream.push_delta(delta)
    }

    fn take_finished_agent_stream(
        &mut self,
        item_id: &str,
        final_text: Option<&str>,
    ) -> Option<AgentStreamFinish> {
        let stream = self.agent_stream.take()?;
        if stream.item_id() != item_id {
            self.agent_stream = Some(stream);
            return None;
        }
        Some(stream.finish_with_final_source(final_text))
    }

    fn flush_live_tail_if_different(&mut self, item_id: &str) -> Vec<HistoryCell> {
        if self
            .live_item
            .as_ref()
            .is_some_and(|live_item| live_item.item_id == item_id)
        {
            return Vec::new();
        }
        let flushed_item = self.live_item.take();
        let flushed_item_id = flushed_item.as_ref().map(|item| item.item_id.clone());
        if let Some(flushed_item_id) = flushed_item_id.as_ref() {
            self.replayed_item_ids.insert(flushed_item_id.clone());
        }
        if self
            .agent_stream
            .as_ref()
            .is_some_and(|stream| Some(stream.item_id()) == flushed_item_id.as_deref())
        {
            self.agent_stream = None;
        }
        if self.should_discard_live_item_on_flush(flushed_item.as_ref()) {
            Vec::new()
        } else {
            flushed_item
                .map(ActiveItemView::into_cell)
                .into_iter()
                .collect::<Vec<_>>()
        }
    }

    fn should_replace_live_tool_placeholder(&self, item: &TranscriptItem) -> bool {
        let Some(live_item) = self.live_item.as_ref() else {
            return false;
        };
        if live_item.kind != TurnItemKind::ToolCall {
            return false;
        }
        let TranscriptItem::ToolResult { tool_name, .. } = item else {
            return false;
        };
        matches!(live_item.body().trim(), "" | "running")
            && live_item.label() == humanize_tool_label(tool_name)
    }

    fn should_discard_live_cell_on_flush(&self) -> bool {
        self.should_discard_live_item_on_flush(self.live_item.as_ref())
    }

    fn should_discard_live_item_on_flush(&self, live_item: Option<&ActiveItemView>) -> bool {
        let Some(live_item) = live_item else {
            return false;
        };
        live_item.kind == TurnItemKind::ToolCall
            && matches!(live_item.body().trim(), "" | "running")
    }

    fn snapshot_effects(&self) -> ActiveTurnEffects {
        self.snapshot_effects_with_replay(Vec::new())
    }

    fn snapshot_effects_with_replay(&self, replay_cells: Vec<HistoryCell>) -> ActiveTurnEffects {
        ActiveTurnEffects {
            active_cell: self
                .agent_stream
                .as_ref()
                .and_then(AgentStreamController::current_live_cell)
                .or_else(|| self.live_item.as_ref().map(ActiveItemView::to_cell)),
            last_copyable_output: self.last_copyable_output.clone(),
            replay_cells,
            consolidate_agent_message: None,
        }
    }
}

fn copyable_output(item: &TranscriptItem) -> Option<String> {
    if let TranscriptItem::AgentMessage { text, .. } = item {
        (!text.trim().is_empty()).then(|| text.clone())
    } else {
        None
    }
}

fn completed_agent_text(item: &TranscriptItem) -> Option<&str> {
    if let TranscriptItem::AgentMessage { text, .. } = item {
        Some(text)
    } else {
        None
    }
}

fn turn_item_kind(item: &TranscriptItem) -> TurnItemKind {
    match item {
        TranscriptItem::SystemMessage { .. } => TurnItemKind::SystemNote,
        TranscriptItem::UserMessage { .. } => TurnItemKind::UserMessage,
        TranscriptItem::AgentMessage { .. } => TurnItemKind::AssistantMessage,
        TranscriptItem::Reasoning { .. } => TurnItemKind::Reasoning,
        TranscriptItem::CommandExecution { .. } => TurnItemKind::CommandExecution,
        TranscriptItem::FileChange { .. } => TurnItemKind::FileChange,
        TranscriptItem::ToolResult { .. } => TurnItemKind::ToolResult,
    }
}

fn should_keep_completed_item_live(item: &TranscriptItem) -> bool {
    if is_web_search_tool_result(item) {
        return false;
    }
    matches!(
        item,
        TranscriptItem::CommandExecution { .. }
            | TranscriptItem::FileChange { .. }
            | TranscriptItem::ToolResult { .. }
    )
}
