use crate::ui::widgets::history_cell::{HistoryCell, Transcript};

#[derive(Default)]
pub(crate) struct CommittedTranscriptStore {
    transcript: Transcript,
    rendered_len: usize,
}

impl CommittedTranscriptStore {
    pub(crate) fn clear(&mut self) {
        self.transcript = Transcript::default();
        self.rendered_len = 0;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.transcript.cells().is_empty()
    }

    pub(crate) fn cells(&self) -> Vec<HistoryCell> {
        self.transcript.cells().to_vec()
    }

    pub(crate) fn append_cells(&mut self, cells: Vec<HistoryCell>) {
        if cells.is_empty() {
            return;
        }

        for cell in cells {
            let _ = self.transcript.push_aggregated(cell);
        }
    }

    pub(crate) fn mark_replayed(&mut self) {
        self.rendered_len = self.transcript.cells().len();
    }

    #[cfg(test)]
    pub(crate) fn pending_cells(&self) -> Vec<HistoryCell> {
        self.transcript.cells()[self.rendered_len..].to_vec()
    }

    pub(crate) fn drain_unrendered_tail(&mut self) -> Vec<HistoryCell> {
        let all_cells = self.transcript.cells();
        let start = self.rendered_len.min(all_cells.len());
        let cells = all_cells[start..]
            .iter()
            .filter(|cell| !cell.body().trim().is_empty())
            .cloned()
            .collect::<Vec<_>>();
        self.rendered_len = all_cells.len();
        cells
    }
}
