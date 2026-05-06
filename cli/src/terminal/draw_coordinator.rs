use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use std::io;

use crate::terminal::Frame;
use crate::terminal::custom_terminal::Terminal;
use crate::terminal::insert_history::insert_history_cells;
use crate::ui::widgets::history_cell::HistoryCell;

pub(crate) struct DrawCoordinator<'a> {
    terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>,
}

impl<'a> DrawCoordinator<'a> {
    pub(crate) fn new(terminal: &'a mut Terminal<CrosstermBackend<io::Stdout>>) -> Self {
        Self { terminal }
    }

    pub(crate) fn draw_frame(
        &mut self,
        height: u16,
        pending_history_cells: Vec<HistoryCell>,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        // The first transition out of the welcome/fullscreen state still needs the
        // viewport established before any history insert. After the history region
        // exists, append committed cells against the current stable boundary first,
        // then adjust the active viewport.
        if self.terminal.viewport_area.top() == 0 {
            self.terminal.ensure_viewport_height(height)?;
            insert_history_cells(self.terminal, pending_history_cells)?;
        } else {
            insert_history_cells(self.terminal, pending_history_cells)?;
            self.terminal.ensure_viewport_height(height)?;
        }
        self.terminal.draw(render)?;
        Ok(())
    }
}
