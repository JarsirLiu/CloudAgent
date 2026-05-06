use crate::app::TuiApp;
use crate::ui::widgets::history_cell::HistoryCell;

impl TuiApp {
    pub(crate) fn push_live_cell(&mut self, cell: HistoryCell) {
        self.transcript_owner.push_live_cell(cell);
    }

    #[cfg(test)]
    pub(crate) fn live_cells(&self) -> &[HistoryCell] {
        self.transcript_owner.live_cells()
    }

    pub(crate) fn drain_pending_history_cells(
        &mut self,
    ) -> Vec<crate::ui::widgets::history_cell::HistoryCell> {
        self.transcript_owner.drain_pending_history_cells()
    }
}
