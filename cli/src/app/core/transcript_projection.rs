use crate::app::core::transcript_owner::TranscriptOwner;
use crate::ui::history_cell::HistoryCell;

#[derive(Clone, Debug)]
pub(crate) struct TranscriptViewportSnapshot {
    pub(crate) revision: u64,
    pub(crate) cells: Vec<HistoryCell>,
}

#[derive(Clone, Debug)]
pub(crate) struct TranscriptScrollbackSnapshot {
    pub(crate) revision: u64,
    pub(crate) cells: Vec<HistoryCell>,
}

pub(crate) fn build_viewport_snapshot(owner: &TranscriptOwner) -> TranscriptViewportSnapshot {
    TranscriptViewportSnapshot {
        revision: owner.live_revision(),
        cells: owner.live_cells_ref().to_vec(),
    }
}

pub(crate) fn build_scrollback_snapshot(owner: &TranscriptOwner) -> TranscriptScrollbackSnapshot {
    TranscriptScrollbackSnapshot {
        revision: owner.committed_revision(),
        cells: owner.committed_cells_ref().to_vec(),
    }
}
