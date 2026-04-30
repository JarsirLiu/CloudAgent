use anyhow::Result;
use ratatui::text::Line;

use crate::terminal::TerminalGuard;
use crate::terminal::insert_history::insert_history_lines;
use crate::ui::widgets::history_cell::HistoryCell;

const TRANSCRIPT_WIDTH_FLOOR: usize = 40;
const TRANSCRIPT_WIDTH_PADDING: usize = 4;

pub(crate) struct ScrollbackSurface {
    inserted_cells: usize,
    rendered_width: Option<u16>,
}

impl ScrollbackSurface {
    pub(crate) fn new() -> Self {
        Self {
            inserted_cells: 0,
            rendered_width: None,
        }
    }

    pub(crate) fn pending_lines(
        &mut self,
        terminal: &TerminalGuard,
        cells: Vec<HistoryCell>,
    ) -> Result<Vec<Line<'static>>> {
        self.rendered_width = Some(terminal.terminal.size()?.width);
        let mut lines = Vec::new();
        for cell in cells {
            lines.extend(self.cell_lines(terminal, &cell)?);
            self.inserted_cells += 1;
        }
        Ok(lines)
    }

    pub(crate) fn reflow_if_width_changed(
        &mut self,
        terminal: &mut TerminalGuard,
        cells: &[HistoryCell],
    ) -> Result<()> {
        let width = terminal.terminal.size()?.width;
        let Some(previous_width) = self.rendered_width else {
            self.rendered_width = Some(width);
            return Ok(());
        };
        if previous_width == width {
            return Ok(());
        }
        self.rendered_width = Some(width);
        if self.inserted_cells == 0 && cells.is_empty() {
            return Ok(());
        }
        self.replace_all(terminal, cells)?;
        Ok(())
    }

    pub(crate) fn replace_all(
        &mut self,
        terminal: &mut TerminalGuard,
        cells: &[HistoryCell],
    ) -> Result<()> {
        terminal.terminal.clear_scrollback_and_visible_screen()?;
        let size = terminal.terminal.size()?;
        terminal
            .terminal
            .set_viewport_area(ratatui::layout::Rect::new(
                0,
                size.height.saturating_sub(1),
                size.width,
                1,
            ));
        terminal.terminal.clear()?;
        self.inserted_cells = 0;
        for cell in cells {
            let lines = self.cell_lines(terminal, cell)?;
            insert_history_lines(&mut terminal.terminal, lines)?;
            self.inserted_cells += 1;
        }
        terminal.terminal.invalidate_viewport();
        Ok(())
    }

    fn cell_lines(
        &self,
        terminal: &TerminalGuard,
        cell: &HistoryCell,
    ) -> Result<Vec<Line<'static>>> {
        let width = terminal
            .terminal
            .size()?
            .width
            .saturating_sub(TRANSCRIPT_WIDTH_PADDING as u16)
            .max(TRANSCRIPT_WIDTH_FLOOR as u16) as usize;
        let mut lines = cell.to_lines_with_mode(width);
        if lines.last().is_some_and(|line| !line.spans.is_empty()) {
            lines.push(Line::raw(""));
        }
        Ok(lines)
    }
}
