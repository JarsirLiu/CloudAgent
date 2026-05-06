use std::fmt;

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::Print;

use ratatui::backend::Backend;
use std::io::Write;

use crate::terminal::custom_terminal::Terminal;

pub(crate) fn update_inline_viewport_area<B>(terminal: &mut Terminal<B>, height: u16) -> Result<()>
where
    B: Backend + Write,
{
    let size = terminal.size()?;
    let previous = terminal.viewport_area;
    let mut area = previous;
    area.height = height.clamp(1, size.height.max(1));
    area.width = size.width;
    area.y = size.height.saturating_sub(area.height);

    if area.y < previous.y {
        let grow_by = previous.y - area.y;
        scroll_region_up(terminal, 0..previous.top(), grow_by)?;
    }

    if area.bottom() > size.height {
        let scroll_by = area.bottom() - size.height;
        scroll_region_up(terminal, 0..area.top(), scroll_by)?;
        area.y = size.height - area.height;
    }

    if area != previous {
        terminal.set_viewport_area(area);
        terminal.clear_rows(previous.y.min(area.y), size.height)?;
    }
    Ok(())
}

fn scroll_region_up<B>(
    terminal: &mut Terminal<B>,
    region: std::ops::Range<u16>,
    scroll_by: u16,
) -> Result<()>
where
    B: Backend + Write,
{
    if scroll_by == 0 || region.is_empty() {
        return Ok(());
    }
    let writer = terminal.backend_mut();
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
