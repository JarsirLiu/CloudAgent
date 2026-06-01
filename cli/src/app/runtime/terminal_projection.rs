use crate::app::TuiApp;
use crate::terminal::PreparedHistoryProjection;
use crate::terminal::TerminalGuard;
use crate::ui::chat_surface::ChatSurface;
use anyhow::Result;
use ratatui::layout::Rect;

#[derive(Default)]
pub(crate) struct TerminalProjectionController;

impl TerminalProjectionController {
    pub(crate) fn reset(&mut self) {}

    pub(crate) fn draw_frame(
        &mut self,
        app: &mut TuiApp,
        terminal: &mut TerminalGuard,
    ) -> Result<()> {
        let size = terminal.terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        let viewport_height = ChatSurface::desired_viewport_height(app, area);
        let projection = PreparedHistoryProjection { viewport_height };
        terminal.draw_projection(projection, |frame| app.render(frame))?;
        Ok(())
    }
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
