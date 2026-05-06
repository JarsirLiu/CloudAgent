use crate::app::TuiApp;
use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone};

impl TuiApp {
    pub(crate) fn push_cell(&mut self, cell: HistoryCell) {
        let _ = self.transcript_state.transcript.push_live(cell);
    }

    pub(crate) fn replace_history_cells(&mut self, cells: Vec<HistoryCell>) {
        let mut cells = cells;
        for cell in &mut cells {
            if matches!(
                cell.tone,
                HistoryTone::Reasoning
                    |
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
    }

    pub(crate) fn history_cells(&self) -> &[HistoryCell] {
        self.transcript_state.transcript.cells()
    }
}
