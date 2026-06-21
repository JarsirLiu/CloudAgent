use crate::app::conversation::projection::{HistoryTurnCells, project_conversation_history};
use crate::app::core::active_cell_controller::ActiveCellController;
use crate::app::core::active_turn::ConsolidateAgentMessage;
use crate::app::core::committed_transcript_store::CommittedTranscriptStore;
use crate::app::core::transcript_projection::{
    TranscriptScrollbackSnapshot, TranscriptViewportSnapshot, build_scrollback_snapshot,
    build_viewport_snapshot,
};
use crate::ui::history_cell::HistoryCell;
use crate::ui::transcript_render_cache::TranscriptRenderCacheKey;
use agent_core::conversation::{ConversationTurn, InputItem, TranscriptItem};
use agent_core::turn::{TurnId, TurnState};
use agent_core::{RuntimeItem, RuntimeItemMetrics, RuntimeItemProgress};
use std::collections::HashSet;

#[derive(Default)]
pub(crate) struct TranscriptOwner {
    committed_store: CommittedTranscriptStore,
    active_cell_controller: ActiveCellController,
    emitted_turn_ids: HashSet<String>,
}

impl TranscriptOwner {
    pub(crate) fn clear(&mut self) {
        self.committed_store.clear();
        self.active_cell_controller.clear();
        self.emitted_turn_ids.clear();
    }

    pub(crate) fn push_live_cell(&mut self, cell: HistoryCell) {
        self.active_cell_controller.replace_notice_cell(cell);
    }

    pub(crate) fn push_committed_cell(&mut self, cell: HistoryCell) {
        self.queue_history_cells(vec![cell]);
    }

    pub(crate) fn replace_live_cells(&mut self, cells: Vec<HistoryCell>, expand_details: bool) {
        self.active_cell_controller
            .replace_live_cells(cells, expand_details);
    }

    #[cfg(test)]
    pub(crate) fn live_cells(&self) -> &[HistoryCell] {
        self.active_cell_controller.live_cells()
    }

    #[cfg(test)]
    pub(crate) fn active_cell(&self) -> Option<&HistoryCell> {
        self.active_cell_controller.active_cell()
    }

    pub(crate) fn live_is_empty(&self) -> bool {
        self.active_cell_controller.is_empty()
    }

    pub(crate) fn has_transcript_content(&self) -> bool {
        !self.live_is_empty() || !self.committed_store.is_empty()
    }

    pub(crate) fn committed_revision(&self) -> u64 {
        self.committed_store.revision()
    }

    pub(crate) fn live_revision(&self) -> u64 {
        self.active_cell_controller.revision()
    }

    pub(crate) fn render_cache_key(&self, width: usize) -> TranscriptRenderCacheKey {
        TranscriptRenderCacheKey {
            live_revision: self.live_revision(),
            width,
        }
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
    pub(crate) fn committed_history_cells(&self) -> Vec<HistoryCell> {
        self.committed_store.cells_ref().to_vec()
    }

    #[cfg(test)]
    pub(crate) fn pending_history_cells(&self) -> Vec<HistoryCell> {
        self.committed_store.cells_ref().to_vec()
    }

    pub(crate) fn viewport_snapshot(&self) -> TranscriptViewportSnapshot {
        build_viewport_snapshot(self)
    }

    pub(crate) fn scrollback_snapshot(&self) -> TranscriptScrollbackSnapshot {
        build_scrollback_snapshot(self)
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

    pub(crate) fn start_item(&mut self, turn_id: TurnId, item: RuntimeItem, expand_details: bool) {
        let effects = self
            .active_cell_controller
            .start_item(turn_id, item, expand_details);
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

    pub(crate) fn append_tool_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) {
        let effects =
            self.active_cell_controller
                .append_tool_delta(turn_id, item_id, delta, expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn append_patch_delta(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        delta: String,
        expand_details: bool,
    ) {
        let effects =
            self.active_cell_controller
                .append_patch_delta(turn_id, item_id, delta, expand_details);
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn update_item_progress(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        progress: RuntimeItemProgress,
        expand_details: bool,
    ) {
        let effects = self.active_cell_controller.update_item_progress(
            turn_id,
            item_id,
            progress,
            expand_details,
        );
        self.apply_active_turn_effects(effects);
    }

    pub(crate) fn update_item_metrics(
        &mut self,
        turn_id: TurnId,
        item_id: String,
        metrics: RuntimeItemMetrics,
        expand_details: bool,
    ) {
        let effects = self.active_cell_controller.update_item_metrics(
            turn_id,
            item_id,
            metrics,
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

    pub(crate) fn active_turn_id(&self) -> Option<&str> {
        self.active_cell_controller.active_turn_id()
    }

    pub(crate) fn committed_cells_ref(&self) -> &[HistoryCell] {
        self.committed_store.cells_ref()
    }

    pub(crate) fn live_cells_ref(&self) -> &[HistoryCell] {
        self.active_cell_controller.live_cells()
    }

    fn queue_history_cells(&mut self, cells: Vec<HistoryCell>) {
        self.committed_store.append_cells(cells);
    }

    fn apply_active_turn_effects(
        &mut self,
        effects: crate::app::core::active_cell_controller::AppliedActiveTurnEffects,
    ) {
        self.queue_history_cells(effects.replay_cells);
        if let Some(message) = effects.consolidate_agent_message {
            self.consolidate_agent_message(message);
        }
    }

    fn consolidate_agent_message(&mut self, message: ConsolidateAgentMessage) {
        self.committed_store
            .consolidate_agent_message(&message.item_id, message.cell);
    }
}
