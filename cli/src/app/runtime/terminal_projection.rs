use crate::app::TuiApp;
use crate::terminal::HistoryReplayBatch;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::TerminalGuard;
use crate::ui::chat_surface::{ChatSurface, TranscriptRenderMetrics};
use crate::ui::history_cell::HistoryCell;
use crate::ui::transcript_line_builder::{
    HistoryCellGapKey, TranscriptLineOptions, build_transcript_lines,
};
use anyhow::Result;

#[derive(Default)]
pub(crate) struct TerminalProjectionController {
    last_scrollback_revision: Option<u64>,
    last_scrollback_metrics: Option<TranscriptRenderMetrics>,
    last_scrollback_cells: Vec<HistoryCell>,
}

impl TerminalProjectionController {
    pub(crate) fn reset(&mut self) {
        self.last_scrollback_revision = None;
        self.last_scrollback_metrics = None;
        self.last_scrollback_cells.clear();
    }

    pub(crate) fn draw_frame(
        &mut self,
        app: &mut TuiApp,
        terminal: &mut TerminalGuard,
    ) -> Result<()> {
        let size = terminal.terminal.size()?;
        let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
        let viewport_height = ChatSurface::desired_viewport_height(app, area);
        let render_metrics = ChatSurface::transcript_render_metrics_for_area(area);
        let projection = PreparedHistoryProjection {
            viewport_height,
            history_update: self.prepare_history_update(app, render_metrics, viewport_height),
        };
        terminal.draw_projection(projection, |frame| app.render(frame))?;
        Ok(())
    }

    fn prepare_history_update(
        &mut self,
        app: &mut TuiApp,
        render_metrics: TranscriptRenderMetrics,
        _viewport_height: u16,
    ) -> Option<HistoryReplayBatch> {
        let snapshot = app.transcript_owner.scrollback_snapshot();
        let revision = snapshot.revision;
        if self.last_scrollback_revision == Some(revision)
            && self.last_scrollback_metrics == Some(render_metrics)
        {
            return None;
        }

        let cells = snapshot.cells;
        let update = scrollback_diff(&self.last_scrollback_cells, &cells);
        let full_replay = self.last_scrollback_metrics != Some(render_metrics)
            || matches!(update, ScrollbackDiff::Replay);
        let update_cells = match update {
            ScrollbackDiff::None if full_replay => cells.clone(),
            ScrollbackDiff::None => Vec::new(),
            ScrollbackDiff::AppendFrom(index) if !full_replay => cells[index..].to_vec(),
            ScrollbackDiff::AppendFrom(_) | ScrollbackDiff::Replay => cells.clone(),
        };

        self.last_scrollback_revision = Some(revision);
        self.last_scrollback_metrics = Some(render_metrics);
        self.last_scrollback_cells = cells;

        let previous_cell = match update {
            ScrollbackDiff::AppendFrom(index) if index > 0 && !full_replay => self
                .last_scrollback_cells
                .get(index - 1)
                .map(HistoryCellGapKey::from_cell),
            _ => None,
        };
        let lines = build_transcript_lines(
            &update_cells,
            TranscriptLineOptions::scrollback(render_metrics.width, previous_cell),
        )
        .lines;
        if lines.is_empty() {
            None
        } else {
            Some(if full_replay {
                HistoryReplayBatch::full_replay(lines, render_metrics.left_padding)
            } else {
                HistoryReplayBatch::append(lines, render_metrics.left_padding)
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollbackDiff {
    None,
    AppendFrom(usize),
    Replay,
}

fn scrollback_diff(previous: &[HistoryCell], current: &[HistoryCell]) -> ScrollbackDiff {
    if current == previous {
        return ScrollbackDiff::None;
    }
    if current.len() >= previous.len()
        && previous
            .iter()
            .zip(current.iter())
            .all(|(previous, current)| previous == current)
    {
        return ScrollbackDiff::AppendFrom(previous.len());
    }
    ScrollbackDiff::Replay
}

pub(crate) fn draw_with_terminal_projection(
    app: &mut TuiApp,
    terminal: &mut TerminalGuard,
) -> Result<()> {
    let mut projection = std::mem::take(&mut app.terminal_projection);
    let result = projection.draw_frame(app, terminal);
    app.terminal_projection = projection;
    result
}

#[cfg(test)]
#[path = "terminal_projection_tests.rs"]
mod tests;
