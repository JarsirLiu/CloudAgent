use crate::ui::widgets::history_cell::{HistoryCell, HistoryTone, Transcript};

#[derive(Default)]
pub(crate) struct LiveTranscript {
    transcript: Transcript,
    last_copyable_output: Option<String>,
}

impl LiveTranscript {
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn push_cell(&mut self, cell: HistoryCell) {
        let _ = self.transcript.push_live(cell);
    }

    pub(crate) fn replace_cells(&mut self, mut cells: Vec<HistoryCell>, expand_details: bool) {
        for cell in &mut cells {
            if matches!(
                cell.tone,
                HistoryTone::Reasoning
                    | HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.expanded = expand_details;
            }
        }
        self.transcript.replace_cells(cells.clone());
        self.transcript.set_tool_cells_expanded(expand_details);
    }

    pub(crate) fn set_expand_details(&mut self, expand_details: bool) {
        self.transcript.set_tool_cells_expanded(expand_details);
    }

    pub(crate) fn cells(&self) -> &[HistoryCell] {
        self.transcript.cells()
    }

    pub(crate) fn last_copyable_output(&self) -> Option<&str> {
        self.last_copyable_output.as_deref()
    }

    pub(crate) fn set_last_copyable_output(&mut self, text: Option<String>) {
        self.last_copyable_output = text;
    }
}
