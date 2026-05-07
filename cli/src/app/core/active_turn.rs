use crate::ui::widgets::history_cell::{
    HistoryCell, HistoryFormat, HistoryTone, humanize_tool_label,
    render_active_item_placeholder, render_history_entry,
};
use agent_protocol::{TranscriptItem, TurnId, TurnItemKind};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub(crate) enum ActiveTurnAction {
    Clear,
    StartLocalUser {
        user_input: String,
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
    AppendOutputDelta {
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
}

#[derive(Default)]
pub(crate) struct ActiveTurnState {
    turn_id: Option<TurnId>,
    live_item_id: Option<String>,
    live_item_kind: Option<TurnItemKind>,
    live_cell: Option<HistoryCell>,
    last_copyable_output: Option<String>,
    replayed_item_ids: HashSet<String>,
    pending_local_user_input: Option<String>,
}

impl ActiveTurnState {
    pub(crate) fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    pub(crate) fn clear(&mut self) -> ActiveTurnEffects {
        self.turn_id = None;
        self.live_item_id = None;
        self.live_item_kind = None;
        self.live_cell = None;
        self.last_copyable_output = None;
        self.replayed_item_ids.clear();
        self.pending_local_user_input = None;
        ActiveTurnEffects::default()
    }

    pub(crate) fn apply(&mut self, action: ActiveTurnAction) -> ActiveTurnEffects {
        match action {
            ActiveTurnAction::Clear => self.clear(),
            ActiveTurnAction::StartLocalUser { user_input } => {
                self.turn_id = None;
                self.live_item_id = None;
                self.live_item_kind = None;
                self.live_cell = None;
                self.last_copyable_output = None;
                self.replayed_item_ids.clear();
                self.pending_local_user_input = Some(user_input);
                ActiveTurnEffects {
                    active_cell: None,
                    last_copyable_output: None,
                    replay_cells: vec![HistoryCell::user(
                        self.pending_local_user_input
                            .as_deref()
                            .unwrap_or_default()
                            .to_string(),
                    )],
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
                self.live_item_id = Some(item_id);
                self.live_item_kind = Some(kind.clone());
                self.live_cell = Some(render_active_item_placeholder(
                    kind,
                    title.as_deref().unwrap_or(""),
                ));
                self.snapshot_effects_with_replay(replay_cells)
            }
            ActiveTurnAction::AppendAgentDelta {
                turn_id,
                item_id,
                delta,
            } => {
                self.ensure_turn(&turn_id);
                let replay_cells = self.ensure_live_tail(
                    &item_id,
                    HistoryCell::agent("", "responding".to_string(), HistoryFormat::Markdown),
                );
                if let Some(cell) = self.live_cell.as_mut() {
                    if cell.body() == "responding" {
                        cell.replace_body(delta.clone());
                    } else {
                        cell.append_body(&delta);
                    }
                }
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
                let replay_cells = self.ensure_live_tail(
                    &item_id,
                    HistoryCell::reasoning("Reasoning", "thinking"),
                );
                if let Some(cell) = self.live_cell.as_mut() {
                    if cell.body() == "thinking" {
                        cell.replace_body(delta.clone());
                    } else {
                        cell.append_body(&delta);
                    }
                }
                self.snapshot_effects_with_replay(replay_cells)
            }
            ActiveTurnAction::AppendOutputDelta {
                turn_id,
                item_id,
                delta,
            } => {
                self.ensure_turn(&turn_id);
                let replay_cells = self.ensure_live_tail(
                    &item_id,
                    HistoryCell::info("Run command", String::new(), HistoryTone::Control),
                );
                if let Some(cell) = self.live_cell.as_mut() {
                    cell.append_body(&delta);
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
                if self.live_item_id.as_deref() == Some(item_id.as_str()) {
                    self.live_item_id = None;
                    self.live_item_kind = None;
                    self.live_cell = None;
                    if !cell.is_empty() {
                        replay_cells.push(cell);
                    }
                } else if self.should_replace_live_tool_placeholder(&item) {
                    self.live_item_id = None;
                    self.live_item_kind = None;
                    self.live_cell = None;
                    if !cell.is_empty() {
                        replay_cells.push(cell);
                    }
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
                    self.live_cell.take();
                    Vec::new()
                } else {
                    self.live_cell.take().into_iter().collect::<Vec<_>>()
                };
                self.live_item_id = None;
                self.live_item_kind = None;
                let last_copyable_output = self.last_copyable_output.clone();
                self.turn_id = None;
                self.last_copyable_output = None;
                self.replayed_item_ids.clear();
                self.pending_local_user_input = None;
                ActiveTurnEffects {
                    active_cell: None,
                    last_copyable_output,
                    replay_cells,
                }
            }
        }
    }

    fn ensure_turn(&mut self, turn_id: &str) {
        match self.turn_id.as_deref() {
            Some(existing) if existing == turn_id => {}
            Some(_) => {
                self.turn_id = Some(turn_id.to_string());
                self.live_item_id = None;
                self.live_item_kind = None;
                self.live_cell = None;
                self.last_copyable_output = None;
                self.replayed_item_ids.clear();
                self.pending_local_user_input = None;
            }
            None => {
                self.turn_id = Some(turn_id.to_string());
            }
        }
    }

    fn ensure_live_tail(&mut self, item_id: &str, placeholder: HistoryCell) -> Vec<HistoryCell> {
        let replay_cells = self.flush_live_tail_if_different(item_id);
        if self.live_item_id.as_deref() != Some(item_id) {
            self.live_item_id = Some(item_id.to_string());
            self.live_item_kind = None;
            self.live_cell = Some(placeholder);
        } else if self.live_cell.is_none() {
            self.live_cell = Some(placeholder);
        }
        replay_cells
    }

    fn flush_live_tail_if_different(&mut self, item_id: &str) -> Vec<HistoryCell> {
        if self.live_item_id.as_deref() == Some(item_id) {
            return Vec::new();
        }
        let flushed_item_id = self.live_item_id.take();
        if let Some(flushed_item_id) = flushed_item_id.as_ref() {
            self.replayed_item_ids.insert(flushed_item_id.clone());
        }
        let flushed = if self.should_discard_live_cell_on_flush() {
            self.live_cell.take();
            Vec::new()
        } else {
            self.live_cell.take().into_iter().collect::<Vec<_>>()
        };
        self.live_item_id = None;
        self.live_item_kind = None;
        flushed
    }

    fn should_replace_live_tool_placeholder(&self, item: &TranscriptItem) -> bool {
        let Some(live_cell) = self.live_cell.as_ref() else {
            return false;
        };
        if self.live_item_kind != Some(TurnItemKind::ToolCall) {
            return false;
        }
        let TranscriptItem::ToolResult { tool_name, .. } = item else {
            return false;
        };
        matches!(live_cell.body().trim(), "" | "running")
            && live_cell.label() == humanize_tool_label(tool_name)
    }

    fn should_discard_live_cell_on_flush(&self) -> bool {
        let Some(live_cell) = self.live_cell.as_ref() else {
            return false;
        };
        self.live_item_kind == Some(TurnItemKind::ToolCall)
            && matches!(live_cell.body().trim(), "" | "running")
    }

    fn snapshot_effects(&self) -> ActiveTurnEffects {
        self.snapshot_effects_with_replay(Vec::new())
    }

    fn snapshot_effects_with_replay(&self, replay_cells: Vec<HistoryCell>) -> ActiveTurnEffects {
        ActiveTurnEffects {
            active_cell: self.live_cell.clone(),
            last_copyable_output: self.last_copyable_output.clone(),
            replay_cells,
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
