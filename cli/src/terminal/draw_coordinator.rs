use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use std::io;

use crate::terminal::Frame;
use crate::terminal::custom_terminal::Terminal;
use crate::terminal::inline_viewport::update_inline_viewport_area;
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
        update_inline_viewport_area(self.terminal, height)?;
        insert_history_cells(self.terminal, pending_history_cells)?;
        self.terminal.draw(render)?;
        Ok(())
    }
}
