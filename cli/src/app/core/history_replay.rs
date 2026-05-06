use crate::app::conversation::projection::HistoryTurnCells;
use crate::ui::widgets::history_cell::HistoryCell;
use std::collections::{HashSet, VecDeque};

#[derive(Default)]
pub(crate) struct HistoryReplay {
    pending_cells: VecDeque<HistoryCell>,
    emitted_turn_ids: HashSet<String>,
    has_committed_history: bool,
}

impl HistoryReplay {
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn clear_pending(&mut self) {
        self.pending_cells.clear();
    }

    pub(crate) fn queue_cells(&mut self, cells: Vec<HistoryCell>) {
        for cell in cells {
            self.has_committed_history = true;
            self.pending_cells.push_back(cell);
        }
    }

    pub(crate) fn queue_turns(&mut self, turns: Vec<HistoryTurnCells>) {
        for turn in turns {
            if !self.emitted_turn_ids.insert(turn.turn_id) {
                continue;
            }
            for cell in turn.cells {
                self.has_committed_history = true;
                self.pending_cells.push_back(cell);
            }
        }
    }

    pub(crate) fn has_committed_history(&self) -> bool {
        self.has_committed_history
    }

    pub(crate) fn has_pending_cells(&self) -> bool {
        !self.pending_cells.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn pending_cells(&self) -> &VecDeque<HistoryCell> {
        &self.pending_cells
    }

    pub(crate) fn drain_cells(&mut self) -> Vec<HistoryCell> {
        let mut cells = Vec::new();
        while let Some(cell) = self.pending_cells.pop_front() {
            if !cell.body().trim().is_empty() {
                cells.push(cell);
            }
        }
        cells
    }
}
