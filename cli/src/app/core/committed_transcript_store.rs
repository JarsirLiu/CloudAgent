use crate::ui::widgets::history_cell::{HistoryCell, Transcript, tool_aggregation};

#[derive(Default)]
pub(crate) struct CommittedTranscriptStore {
    transcript: Transcript,
    revision: u64,
}

impl CommittedTranscriptStore {
    pub(crate) fn clear(&mut self) {
        self.transcript = Transcript::default();
        self.bump_revision();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.transcript.cells().is_empty()
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
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

    pub(crate) fn consolidate_agent_message(
        &mut self,
        item_id: &str,
        final_cell: HistoryCell,
    ) -> bool {
        let Some(start) = self.agent_message_stream_start(item_id) else {
            self.append_cell(final_cell);
            return false;
        };

        let cells = self.transcript.cells_mut();
        let mut remove_indices = cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                is_provisional_agent_message_cell_for(cell, item_id).then_some(index)
            })
            .collect::<Vec<_>>();
        for index in remove_indices.drain(..).rev() {
            cells.remove(index);
        }
        let insert_at = start.min(cells.len());
        cells.insert(insert_at, final_cell);
        self.bump_revision();
        true
    }

    fn append_cell(&mut self, cell: HistoryCell) {
        let mut cell = cell;
        self.apply_append_policy(&mut cell);
        if let Some(last) = self.transcript.cells_mut().last_mut()
            && tool_aggregation::coalesce_agent_stream(last, &cell)
        {
            self.bump_revision();
            return;
        }
        if let Some(last) = self.transcript.cells_mut().last_mut()
            && tool_aggregation::coalesce_tool_like(last, &cell, true)
        {
            self.bump_revision();
            return;
        }
        self.transcript.push(cell);
        self.bump_revision();
    }

    fn agent_message_stream_start(&self, item_id: &str) -> Option<usize> {
        self.transcript
            .cells()
            .iter()
            .position(|cell| is_provisional_agent_message_cell_for(cell, item_id))
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

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

fn is_provisional_agent_message_cell_for(cell: &HistoryCell, item_id: &str) -> bool {
    cell.tone == crate::ui::widgets::history_cell::HistoryTone::Agent
        && cell.kind() == crate::ui::widgets::history_cell::HistoryKind::Message
        && cell.is_provisional_stream()
        && cell.stream_item_id() == Some(item_id)
}
