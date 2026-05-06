pub mod custom_terminal;
pub mod events;
mod inline_viewport;
mod insert_history;

use anyhow::Result;
use crossterm::SynchronizedUpdate;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::text::Line;
use std::io::{self, stdout};
use std::panic;
use std::sync::Once;

pub(crate) use custom_terminal::Frame;
pub(crate) use events::{UiEvent, spawn_tui_event_loop};
use inline_viewport::update_inline_viewport;

static INSTALL_PANIC_HOOK: Once = Once::new();

pub(crate) struct TerminalGuard {
    pub(crate) terminal: custom_terminal::Terminal<CrosstermBackend<io::Stdout>>,
}

pub(crate) fn init() -> Result<TerminalGuard> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let init_result = (|| -> Result<TerminalGuard> {
        execute!(stdout, EnableBracketedPaste)?;
        execute!(stdout, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = custom_terminal::Terminal::new(backend)?;
        Ok(TerminalGuard { terminal })
    })();
    if init_result.is_err() {
        let _ = restore();
    }
    init_result
}

pub(crate) fn restore() -> Result<()> {
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    let _ = execute!(io::stdout(), DisableMouseCapture);
    disable_raw_mode()?;
    Ok(())
}

pub fn install_panic_hook() {
    INSTALL_PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = restore();
            previous(info);
        }));
    });
}

impl TerminalGuard {
    pub(crate) fn new() -> Result<Self> {
        init()
    }

    pub(crate) fn draw_with_history(
        &mut self,
        height: u16,
        pending_history_lines: Vec<Line<'static>>,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        stdout().sync_update(|_| {
            update_inline_viewport(self, height)?;
            insert_history::insert_history_lines(&mut self.terminal, pending_history_lines)?;
            self.terminal.draw(render)?;
            Ok::<(), anyhow::Error>(())
        })??;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = restore();
    }
}
