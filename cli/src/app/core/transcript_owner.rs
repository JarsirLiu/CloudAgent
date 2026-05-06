use crate::app::conversation::projection::{HistoryTurnCells, project_conversation_history};
use crate::app::core::active_turn::{ActiveTurnAction, ActiveTurnEffects, ActiveTurnState};
use crate::app::core::history_replay::HistoryReplay;
use crate::app::core::live_transcript::LiveTranscript;
use crate::ui::widgets::history_cell::{HistoryCell, RenderContext, render_history_entry};
use agent_protocol::{ConversationTurn, TranscriptItem, TurnId, TurnItemKind, TurnState};
#[derive(Default)]
pub(crate) struct TranscriptOwner {
    live_transcript: LiveTranscript,
    active_turn: ActiveTurnState,
    history_replay: HistoryReplay,
}

impl TranscriptOwner {
    pub(crate) fn clear(&mut self) {
        self.live_transcript.clear();
        let _ = self.active_turn.clear();
        self.history_replay.clear();
    }

    pub(crate) fn push_live_cell(&mut self, cell: HistoryCell) {
        self.live_transcript.push_cell(cell);
    }

    pub(crate) fn replace_live_cells(&mut self, cells: Vec<HistoryCell>, expand_details: bool) {
        self.live_transcript.replace_cells(cells, expand_details);
    }

    pub(crate) fn live_cells(&self) -> &[HistoryCell] {
        self.live_transcript.cells()
    }

    pub(crate) fn live_is_empty(&self) -> bool {
        self.live_transcript.cells().is_empty()
    }

    pub(crate) fn has_transcript_content(&self) -> bool {
        !self.live_is_empty()
            || self.history_replay.has_pending_cells()
            || self.history_replay.has_committed_history()
    }

    pub(crate) fn set_expand_details(&mut self, expand_details: bool) {
        self.live_transcript.set_expand_details(expand_details);
    }

    pub(crate) fn last_copyable_output(&self) -> Option<&str> {
        self.live_transcript.last_copyable_output()
    }

    pub(crate) fn set_last_copyable_output(&mut self, text: Option<String>) {
        self.live_transcript.set_last_copyable_output(text);
    }

    pub(crate) fn queue_projected_history(&mut self, turns: Vec<HistoryTurnCells>) {
        self.history_replay.queue_turns(turns);
    }

    pub(crate) fn clear_pending_history(&mut self) {
        self.history_replay.clear_pending();
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
        self.history_replay.pending_cells()
    }

    pub(crate) fn drain_pending_history_cells(&mut self) -> Vec<HistoryCell> {
        self.history_replay.drain_cells()
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
        self.replace_live_cells(effects.live_cells, expand_details);
        self.live_transcript
            .set_last_copyable_output(effects.last_copyable_output);
        self.history_replay.queue_cells(effects.replay_cells);
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

        self.replace_live_cells(live_cells, expand_details);
        self.live_transcript
            .set_last_copyable_output(last_copyable_output.take());
        self.history_replay.queue_cells(replay_cells);
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
