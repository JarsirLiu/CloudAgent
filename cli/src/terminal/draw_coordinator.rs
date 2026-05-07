use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use std::io;

use crate::terminal::Frame;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::PreparedHistoryUpdate;
use crate::terminal::custom_terminal::Terminal;
use crate::terminal::insert_history_lines_raw;

pub(crate) struct DrawCoordinator<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>,
}

impl<'a> DrawCoordinator<'a> {
    pub(crate) fn new(terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>) -> Self {
        Self { terminal }
    }

    pub(crate) fn draw_frame(
        &mut self,
        projection: PreparedHistoryProjection,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        let PreparedHistoryProjection {
            viewport_height,
            history_update,
        } = projection;

        match history_update {
            PreparedHistoryUpdate::ReplayAll(committed_history) => {
                self.terminal.clear_scrollback_and_visible_screen_ansi()?;
                self.terminal.ensure_viewport_height(viewport_height)?;
                insert_history_lines_raw(self.terminal, committed_history)?;
            }
            PreparedHistoryUpdate::AppendTail(committed_tail) => {
                // The first transition out of the welcome/fullscreen state still needs the
                // viewport established before any history insert. After the history region
                // exists, append committed cells against the current stable boundary first,
                // then adjust the active viewport.
                if self.terminal.viewport_area.top() == 0 {
                    self.terminal.ensure_viewport_height(viewport_height)?;
                    insert_history_lines_raw(self.terminal, committed_tail)?;
                } else {
                    insert_history_lines_raw(self.terminal, committed_tail)?;
                    self.terminal.ensure_viewport_height(viewport_height)?;
                }
            }
        }
        self.terminal.draw(render)?;
        Ok(())
    }
}
