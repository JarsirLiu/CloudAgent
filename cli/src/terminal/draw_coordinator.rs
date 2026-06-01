use anyhow::Result;
use ratatui::backend::Backend;
use std::io::Write;

use crate::terminal::Frame;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::custom_terminal::Terminal;

pub(crate) struct DrawCoordinator<'a, B>
where
    B: Backend + Write,
{
    terminal: &'a mut Terminal<B>,
}

impl<'a, B> DrawCoordinator<'a, B>
where
    B: Backend + Write,
{
    pub(crate) fn new(terminal: &'a mut Terminal<B>) -> Self {
        Self { terminal }
    }

    pub(crate) fn draw_frame(
        &mut self,
        projection: PreparedHistoryProjection,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        self.terminal
            .ensure_viewport_height(projection.viewport_height)?;
        self.terminal.draw(render)?;
        Ok(())
    }
}
