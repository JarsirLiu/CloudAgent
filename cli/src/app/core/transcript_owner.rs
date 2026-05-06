use crate::app::conversation::projection::{
    HistoryTurnCells, mark_turn_stream_continuations, project_conversation_history,
};
use crate::app::core::active_turn::{ActiveTurnAction, ActiveTurnEffects, ActiveTurnState};
use crate::ui::widgets::history_cell::{
    HistoryCell, HistoryTone, RenderContext, Transcript, render_history_entry,
};
use agent_protocol::{ConversationTurn, TranscriptItem, TurnId, TurnItemKind, TurnState};
use std::collections::{HashSet, VecDeque};

#[derive(Default)]
pub(crate) struct TranscriptOwner {
    live_transcript: Transcript,
    active_turn: ActiveTurnState,
    pending_history_cells: VecDeque<HistoryCell>,
    emitted_turn_ids: HashSet<String>,
    has_committed_history: bool,
    last_copyable_output: Option<String>,
}

impl TranscriptOwner {
    pub(crate) fn clear(&mut self) {
        self.live_transcript = Transcript::default();
        let _ = self.active_turn.clear();
        self.pending_history_cells.clear();
        self.emitted_turn_ids.clear();
        self.has_committed_history = false;
        self.last_copyable_output = None;
    }

    pub(crate) fn push_live_cell(&mut self, cell: HistoryCell) {
        let _ = self.live_transcript.push_live(cell);
    }

    pub(crate) fn replace_live_cells(&mut self, cells: Vec<HistoryCell>, expand_details: bool) {
        let mut cells = cells;
        for cell in &mut cells {
            if matches!(
                cell.tone,
                HistoryTone::Reasoning
                    | HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.expanded = expand_details;
            }
        }
        self.live_transcript.replace_cells(cells);
        self.live_transcript.set_tool_cells_expanded(expand_details);
    }

    #[cfg(test)]
    pub(crate) fn live_cells(&self) -> &[HistoryCell] {
        self.live_transcript.cells()
    }

    pub(crate) fn active_cell(&self) -> Option<&HistoryCell> {
        self.live_transcript.cells().first()
    }

    pub(crate) fn live_is_empty(&self) -> bool {
        self.live_transcript.is_empty()
    }

    pub(crate) fn has_transcript_content(&self) -> bool {
        !self.live_is_empty()
            || !self.pending_history_cells.is_empty()
            || self.has_committed_history
    }

    pub(crate) fn set_expand_details(&mut self, expand_details: bool) {
        self.live_transcript.set_tool_cells_expanded(expand_details);
    }

    pub(crate) fn last_copyable_output(&self) -> Option<&str> {
        self.last_copyable_output.as_deref()
    }

    pub(crate) fn set_last_copyable_output(&mut self, text: Option<String>) {
        self.last_copyable_output = text;
    }

    pub(crate) fn queue_projected_history(&mut self, turns: Vec<HistoryTurnCells>) {
        for turn in turns {
            if !self.emitted_turn_ids.insert(turn.turn_id) {
                continue;
            }
            self.queue_history_cells(turn.cells);
        }
    }

    pub(crate) fn clear_pending_history(&mut self) {
        self.pending_history_cells.clear();
    }

    pub(crate) fn rebuild_from_history_snapshot(
        &mut self,
        history_snapshot: &[ConversationTurn],
        expand_details: bool,
    ) {
        self.clear();
        if history_snapshot.is_empty() {
            return;
        }

        let projection = project_conversation_history(history_snapshot);
        self.queue_projected_history(projection.completed_cells);
        if let Some(turn) = history_snapshot
            .iter()
            .rev()
            .find(|turn| turn.state == TurnState::Running)
        {
            self.restore_running_turn_snapshot(turn.clone(), expand_details);
        } else {
            self.replace_live_cells(Vec::new(), expand_details);
            self.set_last_copyable_output(projection.last_copyable_output);
        }
    }

    pub(crate) fn clear_active_turn(&mut self, expand_details: bool) {
        self.apply_active_turn(ActiveTurnAction::Clear, expand_details);
    }

    pub(crate) fn start_local_user(&mut self, user_input: String, expand_details: bool) {
        self.apply_active_turn(ActiveTurnAction::StartLocalUser { user_input }, expand_details);
    }

    pub(crate) fn bind_turn_id(&mut self, turn_id: TurnId, expand_details: bool) {
        self.apply_active_turn(ActiveTurnAction::BindTurnId { turn_id }, expand_details);
    }

    pub(crate) fn start_item(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
        expand_details: bool,
    ) {
        self.apply_active_turn(
            ActiveTurnAction::StartItem {
                turn_id,
                item_id,
                kind,
                title,
            },
            expand_details,
        );
    }

    pub(crate) fn append_agent_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) {
        self.apply_active_turn(
            ActiveTurnAction::AppendAgentDelta {
                turn_id,
                item_id,
                delta,
            },
            expand_details,
        );
    }

    pub(crate) fn append_reasoning_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) {
        self.apply_active_turn(
            ActiveTurnAction::AppendReasoningDelta {
                turn_id,
                item_id,
                delta,
            },
            expand_details,
        );
    }

    pub(crate) fn append_output_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) {
        self.apply_active_turn(
            ActiveTurnAction::AppendOutputDelta {
                turn_id,
                item_id,
                delta,
            },
            expand_details,
        );
    }

    pub(crate) fn complete_item(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        item: TranscriptItem,
        expand_details: bool,
    ) {
        self.apply_active_turn(
            ActiveTurnAction::CompleteItem {
                turn_id,
                item_id,
                item,
            },
            expand_details,
        );
    }

    pub(crate) fn complete_turn(&mut self, turn_id: TurnId, expand_details: bool) {
        self.apply_active_turn(ActiveTurnAction::CompleteTurn { turn_id }, expand_details);
    }

    #[cfg(test)]
    pub(crate) fn pending_history_cells(&self) -> &std::collections::VecDeque<HistoryCell> {
        &self.pending_history_cells
    }

    pub(crate) fn drain_pending_history_cells(&mut self) -> Vec<HistoryCell> {
        let mut cells = Vec::new();
        while let Some(cell) = self.pending_history_cells.pop_front() {
            if !cell.body().trim().is_empty() {
                cells.push(cell);
            }
        }
        cells
    }

    fn apply_active_turn(
        &mut self,
        action: ActiveTurnAction,
        expand_details: bool,
    ) {
        let effects = self.active_turn.apply(action);
        self.apply_active_turn_effects(effects, expand_details);
    }

    pub(crate) fn active_turn_id(&self) -> Option<&str> {
        self.active_turn.turn_id()
    }

    fn apply_active_turn_effects(
        &mut self,
        effects: ActiveTurnEffects,
        expand_details: bool,
    ) {
        self.replace_live_cells(effects.active_cell.into_iter().collect(), expand_details);
        self.last_copyable_output = effects.last_copyable_output;
        self.queue_history_cells(effects.replay_cells);
    }

    fn restore_running_turn_snapshot(
        &mut self,
        turn: ConversationTurn,
        expand_details: bool,
    ) {
        let turn_id = turn.id.clone();
        let _ = self.active_turn.clear();
        let _ = self.active_turn.apply(ActiveTurnAction::BindTurnId {
            turn_id: turn_id.clone(),
        });

        let mut replay_cells = Vec::new();
        let mut live_cells = Vec::new();
        let mut last_copyable_output = turn.items.iter().rev().find_map(|item| {
            if let TranscriptItem::AgentMessage { text, .. } = item {
                (!text.trim().is_empty()).then(|| text.clone())
            } else {
                None
            }
        });
        let mut last_live_cell: Option<HistoryCell> = None;
        let mut context = RenderContext;

        for item in turn.items {
            let cell = render_history_entry(&item, &mut context);
            if cell.is_empty() {
                continue;
            }

            if is_live_tail_candidate(&item) {
                if let Some(previous_cell) = last_live_cell.replace(cell) {
                    replay_cells.push(previous_cell);
                }
            } else {
                replay_cells.push(cell);
            }
        }

        if let Some(live_cell) = last_live_cell {
            live_cells.push(live_cell);
        }

        mark_turn_stream_continuations(&mut replay_cells);
        self.replace_live_cells(live_cells, expand_details);
        self.last_copyable_output = last_copyable_output.take();
        self.queue_history_cells(replay_cells);
    }

    fn queue_history_cells(&mut self, cells: Vec<HistoryCell>) {
        if cells.is_empty() {
            return;
        }

        let mut transcript = Transcript::default();
        while let Some(existing) = self.pending_history_cells.pop_front() {
            let _ = transcript.push_aggregated(existing);
        }

        for cell in cells {
            self.has_committed_history = true;
            let _ = transcript.push_aggregated(cell);
        }

        self.pending_history_cells
            .extend(transcript.cells().iter().cloned());
    }
}

fn is_live_tail_candidate(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::AgentMessage { .. }
            | TranscriptItem::Reasoning { .. }
            | TranscriptItem::CommandExecution { .. }
            | TranscriptItem::ToolResult { .. }
            | TranscriptItem::FileChange { .. }
    )
}
