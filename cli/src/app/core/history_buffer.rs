use crate::app::TuiApp;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};

impl TuiApp {
    pub(crate) fn push_cell(&mut self, cell: HistoryCell) {
        let (index, inserted) = self.transcript_state.transcript.push(cell.clone());
        if inserted {
            self.pending_history_cells.push_back(cell);
        } else if let Some(last) = self.pending_history_cells.back_mut() {
            if let Some(coalesced) = self.transcript_state.transcript.cells().get(index) {
                *last = coalesced.clone();
            }
        }
    }

    pub(crate) fn replace_history_cells(&mut self, cells: Vec<HistoryCell>) {
        let mut cells = cells;
        for cell in &mut cells {
            if matches!(
                cell.tone,
                HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.expanded = self.run_state.expand_tool_details;
            }
        }
        self.transcript_state
            .transcript
            .replace_cells(cells.clone());
        self.transcript_state
            .transcript
            .set_tool_cells_expanded(self.run_state.expand_tool_details);
        self.pending_history_cells = cells.into();
        self.pending_history_rebuild = true;
    }

    pub(crate) fn drain_pending_history_cells(&mut self) -> Vec<HistoryCell> {
        self.pending_history_cells.drain(..).collect()
    }

    pub(crate) fn clear_pending_history_cells(&mut self) {
        self.pending_history_cells.clear();
    }

    pub(crate) fn take_pending_history_rebuild(&mut self) -> bool {
        std::mem::take(&mut self.pending_history_rebuild)
    }

    pub(crate) fn history_cells(&self) -> &[HistoryCell] {
        self.transcript_state.transcript.cells()
    }
}
