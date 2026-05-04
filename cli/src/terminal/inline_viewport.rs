use std::fmt;

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::Print;
use ratatui::layout::Position;

use crate::terminal::TerminalGuard;

pub(crate) fn update_inline_viewport(terminal: &mut TerminalGuard, height: u16) -> Result<()> {
    let size = terminal.terminal.size()?;
    let previous = terminal.terminal.viewport_area;
    let mut area = previous;
    area.height = height.clamp(1, size.height.max(1));
    area.width = size.width;

    if area.bottom() > size.height {
        let scroll_by = area.bottom() - size.height;
        scroll_region_up(terminal, 0..area.top(), scroll_by)?;
        area.y = size.height - area.height;
    }

    if area != previous {
        let clear_position = Position::new(0, previous.y.min(area.y));
        terminal.terminal.set_viewport_area(area);
        terminal.terminal.clear_after_position(clear_position)?;
    }
    Ok(())
}

fn scroll_region_up(
    terminal: &mut TerminalGuard,
    region: std::ops::Range<u16>,
    scroll_by: u16,
) -> Result<()> {
    if scroll_by == 0 || region.is_empty() {
        return Ok(());
    }
    let writer = terminal.terminal.backend_mut();
    queue!(writer, SetScrollRegion(region.start + 1..region.end))?;
    queue!(writer, MoveTo(0, region.end.saturating_sub(1)))?;
    for _ in 0..scroll_by {
        queue!(writer, Print("\n"))?;
    }
    queue!(writer, ResetScrollRegion)?;
    std::io::Write::flush(writer)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetScrollRegion(std::ops::Range<u16>);

impl crossterm::Command for SetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[{};{}r", self.0.start, self.0.end)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "SetScrollRegion requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResetScrollRegion;

impl crossterm::Command for ResetScrollRegion {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[r")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "ResetScrollRegion requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}
