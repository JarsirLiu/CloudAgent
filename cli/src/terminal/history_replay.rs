use ratatui::text::Line;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryReplayMode {
    Append,
    FullReplay,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryReplayBatch {
    pub(crate) mode: HistoryReplayMode,
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) left_padding: usize,
}

impl HistoryReplayBatch {
    pub(crate) fn append(lines: Vec<Line<'static>>, left_padding: usize) -> Self {
        Self {
            mode: HistoryReplayMode::Append,
            lines,
            left_padding,
        }
    }

    pub(crate) fn full_replay(lines: Vec<Line<'static>>, left_padding: usize) -> Self {
        Self {
            mode: HistoryReplayMode::FullReplay,
            lines,
            left_padding,
        }
    }
}

#[cfg(test)]
#[path = "history_replay_tests.rs"]
mod tests;
