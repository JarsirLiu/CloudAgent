use anyhow::Result;
use ratatui::Frame;

use crate::terminal::TerminalGuard;

#[allow(dead_code)]
pub(crate) fn draw_frame(
    terminal: &mut TerminalGuard,
    render: impl FnOnce(&mut Frame),
) -> Result<()> {
    terminal.terminal.draw(render)?;
    Ok(())
}
