use crate::app::conversation::projection::{HistoryTurnCells, project_conversation_history};
use crate::app::core::active_cell_controller::ActiveCellController;
use crate::app::core::active_turn::ConsolidateAgentMessage;
use crate::app::core::committed_transcript_store::{
    CommittedTranscriptStore, ProvisionalAgentMessageFootprint,
};
use crate::ui::widgets::history_cell::HistoryCell;
use agent_core::conversation::{ConversationTurn, InputItem, TranscriptItem};
use agent_core::turn::{TurnId, TurnItemKind, TurnState};
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct TranscriptOwner {
    committed_store: CommittedTranscriptStore,
    pending_store: CommittedTranscriptStore,
    active_cell_controller: ActiveCellController,
    emitted_turn_ids: HashSet<String>,
    history_replay_requested: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollbackReflow {
    NotRequired,
    Required,
}

impl ScrollbackReflow {
    fn for_consolidated_agent_message(
        pending_replaced: bool,
        committed_replaced: bool,
        pending_footprint: ProvisionalAgentMessageFootprint,
        committed_footprint: ProvisionalAgentMessageFootprint,
    ) -> Self {
        if committed_replaced
            && (!pending_replaced || committed_footprint.rendered_beyond(pending_footprint))
        {
            Self::Required
        } else {
            Self::NotRequired
        }
    }
}

impl TranscriptOwner {
    pub(crate) fn clear(&mut self) {
        self.committed_store.clear();
        self.pending_store.clear();
        self.active_cell_controller.clear();
        self.emitted_turn_ids.clear();
        self.history_replay_requested = false;
    }

    pub(crate) fn push_live_cell(&mut self, cell: HistoryCell) {
        self.active_cell_controller.replace_notice_cell(cell);
    }

    pub(crate) fn replace_live_cells(&mut self, cells: Vec<HistoryCell>, expand_details: bool) {
        self.active_cell_controller
            .replace_live_cells(cells, expand_details);
    }

    #[cfg(test)]
    pub(crate) fn live_cells(&self) -> &[HistoryCell] {
        self.active_cell_controller.live_cells()
    }

    pub(crate) fn active_cell(&self) -> Option<&HistoryCell> {
        self.active_cell_controller.active_cell()
    }

    pub(crate) fn live_is_empty(&self) -> bool {
        self.active_cell_controller.is_empty()
    }

    pub(crate) fn has_transcript_content(&self) -> bool {
        !self.live_is_empty() || !self.committed_store.is_empty()
    }

    pub(crate) fn set_expand_details(&mut self, expand_details: bool) {
        self.active_cell_controller
            .set_expand_details(expand_details);
    }

    pub(crate) fn last_copyable_output(&self) -> Option<&str> {
        self.active_cell_controller.last_copyable_output()
    }

    pub(crate) fn set_last_copyable_output(&mut self, text: Option<String>) {
        self.active_cell_controller.set_last_copyable_output(text);
    }

    pub(crate) fn queue_projected_history(&mut self, turns: Vec<HistoryTurnCells>) {
        for turn in turns {
            if !self.emitted_turn_ids.insert(turn.turn_id) {
                continue;
            }
            self.queue_history_cells(turn.cells);
        }
    }

    #[cfg(test)]
    pub(crate) fn queue_history_cells_for_test(&mut self, cells: Vec<HistoryCell>) {
        self.queue_history_cells(cells);
    }

    pub(crate) fn committed_history_cells(&self) -> Vec<HistoryCell> {
        self.committed_store.cells()
    }

    pub(crate) fn mark_committed_history_replayed(&mut self) {
        self.pending_store.clear();
    }

    pub(crate) fn take_history_replay_requested(&mut self) -> bool {
        std::mem::take(&mut self.history_replay_requested)
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
            let replay_cells = self
                .active_cell_controller
                .restore_running_turn_snapshot(turn.clone(), expand_details);
            self.queue_history_cells(replay_cells);
        } else {
            self.replace_live_cells(Vec::new(), expand_details);
            self.set_last_copyable_output(projection.last_copyable_output);
        }
    }

    pub(crate) fn clear_active_turn(&mut self, expand_details: bool) {
        let effects = self
            .active_cell_controller
            .clear_active_turn(expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn start_local_user(&mut self, user_input: Vec<InputItem>, expand_details: bool) {
        let effects = self
            .active_cell_controller
            .start_local_user(user_input, expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn bind_turn_id(&mut self, turn_id: TurnId, expand_details: bool) {
        let effects = self
            .active_cell_controller
            .bind_turn_id(turn_id, expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn start_item(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
        expand_details: bool,
    ) {
        let effects =
            self.active_cell_controller
                .start_item(turn_id, item_id, kind, title, expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn append_agent_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) {
        let effects =
            self.active_cell_controller
                .append_agent_delta(turn_id, item_id, delta, expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn append_reasoning_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) {
        let effects = self.active_cell_controller.append_reasoning_delta(
            turn_id,
            item_id,
            delta,
            expand_details,
        );
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn complete_item(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        item: TranscriptItem,
        expand_details: bool,
    ) {
        let effects =
            self.active_cell_controller
                .complete_item(turn_id, item_id, item, expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn complete_turn(&mut self, turn_id: TurnId, expand_details: bool) {
        let effects = self
            .active_cell_controller
            .complete_turn(turn_id, expand_details);
        self.apply_active_turn_effects(effects);
    }

    #[cfg(test)]
    pub(crate) fn pending_history_cells(&self) -> Vec<HistoryCell> {
        self.pending_store.cells()
    }

    pub(crate) fn drain_pending_history_cells(&mut self) -> Vec<HistoryCell> {
        let cells = self
            .pending_store
            .cells()
            .into_iter()
            .filter(|cell| !cell.body().trim().is_empty())
            .collect::<Vec<_>>();
        self.pending_store.clear();
        cells
    }

    pub(crate) fn active_turn_id(&self) -> Option<&str> {
        self.active_cell_controller.active_turn_id()
    }

    fn queue_history_cells(&mut self, cells: Vec<HistoryCell>) {
        self.committed_store.append_cells(cells.clone());
        self.pending_store.append_cells(cells);
    }

    fn apply_active_turn_effects(
        &mut self,
        effects: crate::app::core::active_cell_controller::AppliedActiveTurnEffects,
    ) {
        self.queue_history_cells(effects.replay_cells);
        if let Some(message) = effects.consolidate_agent_message
            && self.consolidate_agent_message(message) == ScrollbackReflow::Required
        {
            self.history_replay_requested = true;
        }
    }

    fn consolidate_agent_message(&mut self, message: ConsolidateAgentMessage) -> ScrollbackReflow {
        let pending_footprint = self
            .pending_store
            .provisional_agent_message_footprint(&message.item_id);
        let committed_footprint = self
            .committed_store
            .provisional_agent_message_footprint(&message.item_id);
        let pending_replaced = self
            .pending_store
            .consolidate_agent_message(&message.item_id, message.cell.clone());
        let committed_replaced = self
            .committed_store
            .consolidate_agent_message(&message.item_id, message.cell);

        ScrollbackReflow::for_consolidated_agent_message(
            pending_replaced,
            committed_replaced,
            pending_footprint,
            committed_footprint,
        )
    }
}
