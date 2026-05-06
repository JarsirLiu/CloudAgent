use crate::app::TuiApp;
use crate::terminal::TerminalGuard;
use crate::ui::chat_surface::ChatSurface;
use anyhow::Result;
use ratatui::layout::Rect;

pub(crate) fn draw_app_frame(app: &mut TuiApp, terminal: &mut TerminalGuard) -> Result<()> {
    let size = terminal.terminal.size()?;
    let area = Rect::new(0, 0, size.width, size.height);
    let viewport_height = ChatSurface::desired_viewport_height(app, area);
    let pending_history_cells = app.drain_pending_history_cells();
    terminal.draw_with_history(viewport_height, pending_history_cells, |frame| {
        app.render(frame)
    })?;
    Ok(())
}
