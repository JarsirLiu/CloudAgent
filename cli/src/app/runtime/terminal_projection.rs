use crate::app::TuiApp;
use crate::terminal::HistoryProjectionUpdate;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::TerminalGuard;
use crate::ui::chat_surface::{ChatSurface, TranscriptRenderMetrics};
use crate::ui::widgets::history_cell::HistoryCell;
use anyhow::Result;
use ratatui::layout::Rect;
use ratatui::text::Line;

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
        let area = Rect::new(0, 0, size.width, size.height);
        let viewport_height = ChatSurface::desired_viewport_height(app, area);
        let render_metrics = ChatSurface::transcript_render_metrics_for_area(area);
        let projection = PreparedHistoryProjection {
            viewport_height,
            history_update: self.prepare_history_update(app, render_metrics),
        };
        terminal.draw_projection(projection, |frame| app.render(frame))?;
        Ok(())
    }

    fn prepare_history_update(
        &mut self,
        app: &mut TuiApp,
        render_metrics: TranscriptRenderMetrics,
    ) -> Option<HistoryProjectionUpdate> {
        let revision = app.transcript_owner.committed_scrollback_revision();
        if self.last_scrollback_revision == Some(revision)
            && self.last_scrollback_metrics == Some(render_metrics)
        {
            return None;
        }

        let cells = app.transcript_owner.committed_cells_for_scrollback();
        let update = scrollback_diff(&self.last_scrollback_cells, &cells);
        let full_replay = self.last_scrollback_metrics != Some(render_metrics)
            || matches!(update, ScrollbackDiff::Replay);
        let update_cells = match update {
            ScrollbackDiff::None => Vec::new(),
            ScrollbackDiff::AppendFrom(index) if !full_replay => cells[index..].to_vec(),
            ScrollbackDiff::AppendFrom(_) | ScrollbackDiff::Replay => cells.clone(),
        };

        self.last_scrollback_revision = Some(revision);
        self.last_scrollback_metrics = Some(render_metrics);
        self.last_scrollback_cells = cells;

        let previous_cell = match update {
            ScrollbackDiff::AppendFrom(index) if index > 0 && !full_replay => {
                self.last_scrollback_cells.get(index - 1)
            }
            _ => None,
        };
        let lines =
            history_cells_to_scrollback_lines(&update_cells, render_metrics.width, previous_cell);
        if lines.is_empty() {
            None
        } else {
            Some(HistoryProjectionUpdate {
                lines,
                full_replay,
                left_padding: render_metrics.left_padding,
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

fn history_cells_to_scrollback_lines(
    cells: &[HistoryCell],
    width: usize,
    previous_cell: Option<&HistoryCell>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for cell in cells {
        if cell.body().trim().is_empty() {
            continue;
        }
        if should_insert_cell_gap(!lines.is_empty(), previous_cell, cell) {
            lines.push(Line::from(""));
        }
        lines.extend(cell.to_transcript_lines(width.max(1)));
    }
    lines
}

fn should_insert_cell_gap(
    emitted_in_batch: bool,
    previous_cell: Option<&HistoryCell>,
    cell: &HistoryCell,
) -> bool {
    if cell.is_stream_continuation() {
        return false;
    }
    emitted_in_batch || previous_cell.is_some_and(|previous| !previous.body().trim().is_empty())
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
mod tests {
    use super::{ScrollbackDiff, history_cells_to_scrollback_lines, scrollback_diff};
    use crate::ui::widgets::history_cell::{HistoryCell, HistoryFormat};

    #[test]
    fn converts_committed_cells_to_scrollback_lines() {
        let cells = vec![
            HistoryCell::user("hello"),
            HistoryCell::agent("assistant", "world", HistoryFormat::Markdown),
        ];

        let lines = history_cells_to_scrollback_lines(&cells, 80, None)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();

        assert!(lines.iter().any(|line| line.contains("hello")));
        assert!(lines.iter().any(|line| line.contains("world")));
    }

    #[test]
    fn empty_committed_cells_do_not_emit_scrollback_update() {
        let mut app = crate::app::TuiApp::new(
            "default".to_string(),
            "test",
            std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent"),
            std::path::PathBuf::from("D:\\learn\\gifti\\cloudagent\\.test-store"),
            false,
            "ReadOnly".to_string(),
        );
        let mut projection = super::TerminalProjectionController::default();

        let update = projection.prepare_history_update(
            &mut app,
            crate::ui::chat_surface::TranscriptRenderMetrics {
                width: 80,
                left_padding: 4,
            },
        );

        assert!(update.is_none());
    }

    #[test]
    fn scrollback_diff_allows_append_only_updates() {
        let previous = vec![HistoryCell::user("hello")];
        let current = vec![
            HistoryCell::user("hello"),
            HistoryCell::agent("assistant", "world", HistoryFormat::Markdown),
        ];

        assert_eq!(
            scrollback_diff(&previous, &current),
            ScrollbackDiff::AppendFrom(1)
        );
    }

    #[test]
    fn scrollback_diff_replays_when_existing_prefix_changes() {
        let previous = vec![
            HistoryCell::user("hello"),
            HistoryCell::agent("assistant", "old", HistoryFormat::Markdown),
        ];
        let current = vec![
            HistoryCell::user("hello"),
            HistoryCell::agent("assistant", "new", HistoryFormat::Markdown),
            HistoryCell::user("next"),
        ];

        assert_eq!(scrollback_diff(&previous, &current), ScrollbackDiff::Replay);
    }

    #[test]
    fn incremental_scrollback_lines_include_gap_after_previous_cell() {
        let previous = HistoryCell::agent("assistant", "answer", HistoryFormat::Markdown);
        let cells = vec![HistoryCell::user("next question")];

        let lines = history_cells_to_scrollback_lines(&cells, 80, Some(&previous))
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();

        assert_eq!(lines.first().map(String::as_str), Some(""));
        assert!(lines.iter().any(|line| line.contains("next question")));
    }
}
