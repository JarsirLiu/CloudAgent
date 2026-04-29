use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{self, Event as CEvent, KeyEvent, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

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

impl TerminalGuard {
    pub(crate) fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        let _ = execute!(stdout, EnableAlternateScroll);
        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(io::stdout(), DisableAlternateScroll);
        let _ = disable_raw_mode();
    }
}

pub(crate) enum UiEvent {
    Key(KeyEvent),
    MouseScroll { up: bool },
    Tick,
}

pub(crate) fn spawn_tui_event_loop() -> mpsc::UnboundedReceiver<UiEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(120)) {
                Ok(true) => match event::read() {
                    Ok(CEvent::Key(key)) => {
                        if tx.send(UiEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CEvent::Mouse(mouse)) => {
                        let scroll = match mouse.kind {
                            MouseEventKind::ScrollUp => Some(true),
                            MouseEventKind::ScrollDown => Some(false),
                            _ => None,
                        };
                        if let Some(up) = scroll
                            && tx.send(UiEvent::MouseScroll { up }).is_err()
                        {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {
                    if tx.send(UiEvent::Tick).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

