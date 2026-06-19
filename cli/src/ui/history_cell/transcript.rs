use super::{HistoryCell, HistoryTone};

#[derive(Default)]
pub struct Transcript {
    cells: Vec<HistoryCell>,
}

impl Transcript {
    pub fn push(&mut self, cell: HistoryCell) {
        self.cells.push(cell);
    }

    pub fn replace_cells(&mut self, cells: Vec<HistoryCell>) {
        self.cells.clear();
        for cell in cells {
            self.push(cell);
        }
    }

    pub fn set_tool_cells_expanded(&mut self, expanded: bool) {
        for cell in &mut self.cells {
            if matches!(
                cell.tone,
                HistoryTone::Reasoning
                    | HistoryTone::Tool
                    | HistoryTone::Control
                    | HistoryTone::Warning
                    | HistoryTone::Error
            ) {
                cell.set_expanded(expanded);
            }
        }
    }

    pub fn cells(&self) -> &[HistoryCell] {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut Vec<HistoryCell> {
        &mut self.cells
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}
