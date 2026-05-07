use crate::ui::widgets::history_cell::{HistoryCell, Transcript, tool_aggregation};

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
            self.append_cell(cell);
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

    fn append_cell(&mut self, cell: HistoryCell) {
        let mut cell = cell;
        self.apply_append_policy(&mut cell);
        if let Some(last) = self.transcript.cells_mut().last_mut()
            && tool_aggregation::coalesce_agent_stream(last, &cell)
        {
            return;
        }
        if let Some(last) = self.transcript.cells_mut().last_mut()
            && tool_aggregation::coalesce_tool_like(last, &cell, true)
        {
            return;
        }
        self.transcript.push(cell);
    }

    fn apply_append_policy(&mut self, cell: &mut HistoryCell) {
        if cell.is_empty() {
            cell.set_stream_continuation(false);
            return;
        }

        let is_agent_message = cell.tone == crate::ui::widgets::history_cell::HistoryTone::Agent
            && cell.kind() == crate::ui::widgets::history_cell::HistoryKind::Message;
        let previous_was_agent_message = self
            .transcript
            .cells()
            .last()
            .map(|previous| {
                previous.tone == crate::ui::widgets::history_cell::HistoryTone::Agent
                    && previous.kind() == crate::ui::widgets::history_cell::HistoryKind::Message
            })
            .unwrap_or(false);
        cell.set_stream_continuation(is_agent_message && previous_was_agent_message);
    }
}
