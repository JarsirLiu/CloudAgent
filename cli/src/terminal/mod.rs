pub mod events;
pub mod frame;

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;

pub(crate) use events::{UiEvent, spawn_tui_event_loop};

pub(crate) struct TerminalGuard {
    pub(crate) terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableAlternateScroll;

impl crossterm::Command for EnableAlternateScroll {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1007h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<(), std::io::Error> {
        Err(std::io::Error::other(
            "EnableAlternateScroll requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableAlternateScroll;

impl crossterm::Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(f, "\x1b[?1007l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<(), std::io::Error> {
        Err(std::io::Error::other(
            "DisableAlternateScroll requires ANSI execution",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

pub(crate) fn init() -> Result<TerminalGuard> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let _ = execute!(stdout, EnableAlternateScroll);
    execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    Ok(TerminalGuard { terminal })
}

pub(crate) fn restore() -> Result<()> {
    let _ = execute!(io::stdout(), DisableAlternateScroll);
    disable_raw_mode()?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn with_restored<F, R>(f: F) -> Result<R>
where
    F: FnOnce() -> Result<R>,
{
    restore()?;
    let result = f();
    let _ = init();
    result
}

impl TerminalGuard {
    pub(crate) fn new() -> Result<Self> {
        init()
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = restore();
    }
}
