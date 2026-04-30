use anyhow::Result;

use crate::terminal::{Frame, TerminalGuard};

#[allow(dead_code)]
pub(crate) fn draw_frame(
    terminal: &mut TerminalGuard,
    render: impl FnOnce(&mut Frame),
) -> Result<()> {
    terminal.terminal.draw(render)?;
    Ok(())
}
