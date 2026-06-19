//! Terminal and event-loop infrastructure for the CLI app.
//!
//! This module owns low-level terminal initialization, rendering primitives,
//! keyboard-mode handling, and the event loop plumbing used by the TUI runtime.
//! The `events/` submodule is an implementation cluster for event polling,
//! frame requests, and cross-thread input plumbing.

mod color_compat;
pub mod custom_terminal;
mod draw_coordinator;
pub mod events;
mod history_replay;
mod keyboard_modes;

use anyhow::Result;
use crossterm::SynchronizedUpdate;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use std::io::{self, IsTerminal, stdout};
use std::panic;
use std::sync::Once;

pub use color_compat::apply_color_cli_preference;
use color_compat::{TerminalCapabilities, prepare_terminal_color_output};
pub(crate) use custom_terminal::Frame;
use draw_coordinator::DrawCoordinator;
pub(crate) use events::{FrameRequester, UiEvent, spawn_tui_event_loop};
pub(crate) use history_replay::{HistoryReplayBatch, HistoryReplayMode};

static INSTALL_PANIC_HOOK: Once = Once::new();

pub(crate) struct TerminalGuard {
    pub(crate) terminal: custom_terminal::Terminal<CrosstermBackend<io::Stdout>>,
    capabilities: TerminalCapabilities,
}

pub(crate) struct PreparedHistoryProjection {
    pub(crate) viewport_height: u16,
    pub(crate) history_update: Option<HistoryReplayBatch>,
}

pub(crate) fn init() -> Result<TerminalGuard> {
    if !io::stdin().is_terminal() {
        anyhow::bail!("stdin is not a terminal");
    }
    if !io::stdout().is_terminal() {
        anyhow::bail!("stdout is not a terminal");
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let init_result = (|| -> Result<TerminalGuard> {
        prepare_terminal_color_output();
        execute!(stdout, EnableBracketedPaste)?;
        keyboard_modes::enable_keyboard_enhancement();
        flush_terminal_input_buffer();
        let backend = CrosstermBackend::new(io::stdout());
        let capabilities = TerminalCapabilities::detect();
        let terminal = custom_terminal::Terminal::new(backend, capabilities)?;
        Ok(TerminalGuard {
            terminal,
            capabilities,
        })
    })();
    if init_result.is_err() {
        let _ = restore();
    }
    init_result
}

#[cfg(unix)]
fn flush_terminal_input_buffer() {
    // Safety: flushing the stdin queue does not move ownership and only drops pending input events.
    let _ = unsafe { libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH) };
}

#[cfg(windows)]
fn flush_terminal_input_buffer() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::FlushConsoleInputBuffer;
    use windows_sys::Win32::System::Console::GetStdHandle;
    use windows_sys::Win32::System::Console::STD_INPUT_HANDLE;

    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return;
    }
    let _ = unsafe { FlushConsoleInputBuffer(handle) };
}

#[cfg(not(any(unix, windows)))]
fn flush_terminal_input_buffer() {}

pub(crate) fn restore() -> Result<()> {
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    keyboard_modes::restore_keyboard_enhancement_stack();
    disable_raw_mode()?;
    Ok(())
}

pub fn install_panic_hook() {
    INSTALL_PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            keyboard_modes::reset_keyboard_reporting_after_exit();
            let _ = restore();
            previous(info);
        }));
    });
}

impl TerminalGuard {
    pub(crate) fn new() -> Result<Self> {
        init()
    }

    pub(crate) fn draw_projection(
        &mut self,
        projection: PreparedHistoryProjection,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        if self.capabilities.supports_synchronized_update {
            stdout().sync_update(|_| {
                let mut coordinator = DrawCoordinator::new(&mut self.terminal);
                coordinator.draw_frame(projection, render)?;
                Ok::<(), anyhow::Error>(())
            })??;
        } else {
            let mut coordinator = DrawCoordinator::new(&mut self.terminal);
            coordinator.draw_frame(projection, render)?;
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        keyboard_modes::reset_keyboard_reporting_after_exit();
        let _ = restore();
    }
}
