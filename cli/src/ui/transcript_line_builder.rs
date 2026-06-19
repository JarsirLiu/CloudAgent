use crate::ui::history_cell::{HistoryCell, HistoryKind};
use ratatui::text::Line;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TranscriptLineMode {
    Live,
    Scrollback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HistoryCellGapKey {
    pub(crate) is_empty: bool,
    pub(crate) is_stream_continuation: bool,
    pub(crate) kind: HistoryKind,
}

impl HistoryCellGapKey {
    pub(crate) fn from_cell(cell: &HistoryCell) -> Self {
        Self {
            is_empty: cell.body().trim().is_empty(),
            is_stream_continuation: cell.is_stream_continuation(),
            kind: cell.kind(),
        }
    }

    fn should_gap_before(&self, previous: Option<HistoryCellGapKey>) -> bool {
        !self.is_empty
            && !self.is_stream_continuation
            && previous.is_some_and(|previous| !previous.is_empty)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TranscriptLineOptions {
    pub(crate) width: usize,
    pub(crate) mode: TranscriptLineMode,
    pub(crate) previous_cell: Option<HistoryCellGapKey>,
    pub(crate) trim_trailing_blank_lines: bool,
}

impl TranscriptLineOptions {
    pub(crate) fn live(width: usize) -> Self {
        Self {
            width,
            mode: TranscriptLineMode::Live,
            previous_cell: None,
            trim_trailing_blank_lines: true,
        }
    }

    pub(crate) fn scrollback(width: usize, previous_cell: Option<HistoryCellGapKey>) -> Self {
        Self {
            width,
            mode: TranscriptLineMode::Scrollback,
            previous_cell,
            trim_trailing_blank_lines: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TranscriptLineBuild {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) last_cell: Option<HistoryCellGapKey>,
}

pub(crate) fn build_transcript_lines(
    cells: &[HistoryCell],
    options: TranscriptLineOptions,
) -> TranscriptLineBuild {
    let mut lines = Vec::new();
    let mut previous_cell = options.previous_cell;
    let mut emitted_any = false;

    let had_previous_context = options.previous_cell.is_some();
    for cell in cells {
        let cell_key = HistoryCellGapKey::from_cell(cell);
        if cell_key.is_empty {
            previous_cell = Some(cell_key);
            continue;
        }
        if (emitted_any || had_previous_context) && should_insert_gap(previous_cell, cell_key) {
            lines.push(Line::from(""));
            if should_insert_extra_tool_gap(previous_cell, cell_key, options.mode) {
                lines.push(Line::from(""));
            }
        }
        lines.extend(build_cell_lines(cell, options.width, options.mode));
        previous_cell = Some(cell_key);
        emitted_any = true;
    }

    if options.trim_trailing_blank_lines {
        trim_trailing_blank_lines(&mut lines);
    }

    TranscriptLineBuild {
        lines,
        last_cell: previous_cell,
    }
}

pub(crate) fn build_cell_lines(
    cell: &HistoryCell,
    width: usize,
    mode: TranscriptLineMode,
) -> Vec<Line<'static>> {
    match mode {
        TranscriptLineMode::Live => cell.to_live_transcript_lines(width),
        TranscriptLineMode::Scrollback => cell.to_transcript_lines(width),
    }
}

fn should_insert_gap(
    previous_cell: Option<HistoryCellGapKey>,
    current_cell: HistoryCellGapKey,
) -> bool {
    current_cell.should_gap_before(previous_cell)
}

fn should_insert_extra_tool_gap(
    previous_cell: Option<HistoryCellGapKey>,
    current_cell: HistoryCellGapKey,
    mode: TranscriptLineMode,
) -> bool {
    match mode {
        TranscriptLineMode::Live => previous_cell.is_some_and(|previous| {
            matches!(
                (previous.kind, current_cell.kind),
                (
                    HistoryKind::Message | HistoryKind::Reasoning | HistoryKind::Exploration,
                    HistoryKind::Command | HistoryKind::Tool
                )
            )
        }),
        TranscriptLineMode::Scrollback => false,
    }
}

fn trim_trailing_blank_lines(lines: &mut Vec<Line<'static>>) {
    while lines
        .last()
        .is_some_and(|line| line.to_string().trim().is_empty())
    {
        lines.pop();
    }
}

#[cfg(test)]
#[path = "transcript_line_builder_tests.rs"]
mod tests;
