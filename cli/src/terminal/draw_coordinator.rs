use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use std::io;

use crate::terminal::HistoryProjection;
use crate::terminal::HistoryUpdate;
use crate::terminal::Frame;
use crate::terminal::custom_terminal::Terminal;
use crate::terminal::insert_history::insert_history_cells;

pub(crate) struct DrawCoordinator<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>,
}

impl<'a> DrawCoordinator<'a> {
    pub(crate) fn new(terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>) -> Self {
        Self { terminal }
    }

    pub(crate) fn draw_frame(
        &mut self,
        projection: HistoryProjection,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        let HistoryProjection {
            viewport_height,
            history_render_width,
            history_update,
        } = projection;

        match history_update {
            HistoryUpdate::ReplayAll(committed_history) => {
                self.terminal.clear_scrollback_and_visible_screen_ansi()?;
                self.terminal.ensure_viewport_height(viewport_height)?;
                insert_history_cells(self.terminal, committed_history, history_render_width)?;
            }
            HistoryUpdate::AppendTail(committed_tail) => {
                // The first transition out of the welcome/fullscreen state still needs the
                // viewport established before any history insert. After the history region
                // exists, append committed cells against the current stable boundary first,
                // then adjust the active viewport.
                if self.terminal.viewport_area.top() == 0 {
                    self.terminal.ensure_viewport_height(viewport_height)?;
                    insert_history_cells(self.terminal, committed_tail, history_render_width)?;
                } else {
                    insert_history_cells(self.terminal, committed_tail, history_render_width)?;
                    self.terminal.ensure_viewport_height(viewport_height)?;
                }
            }
        }
        self.terminal.draw(render)?;
        Ok(())
    }
}
