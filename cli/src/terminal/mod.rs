pub mod custom_terminal;
mod color_compat;
mod draw_coordinator;
pub mod events;
mod insert_history;

use anyhow::Result;
use crossterm::SynchronizedUpdate;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::text::Line;
use std::io::{self, stdout};
use std::panic;
use std::sync::Once;

use crate::ui::widgets::history_cell::HistoryCell;

pub(crate) use custom_terminal::Frame;
use draw_coordinator::DrawCoordinator;
pub(crate) use events::{FrameRequester, UiEvent, spawn_tui_event_loop};
pub(crate) use insert_history::{insert_history_lines_raw, prepare_history_lines};

static INSTALL_PANIC_HOOK: Once = Once::new();

pub(crate) struct TerminalGuard {
    pub(crate) terminal: custom_terminal::Terminal<CrosstermBackend<io::Stdout>>,
}

pub(crate) struct HistoryProjection {
    pub(crate) viewport_height: u16,
    pub(crate) history_render_width: usize,
    pub(crate) history_update: HistoryUpdate,
}

pub(crate) struct PreparedHistoryProjection {
    pub(crate) viewport_height: u16,
    pub(crate) history_update: PreparedHistoryUpdate,
}

pub(crate) enum HistoryUpdate {
    ReplayAll(Vec<HistoryCell>),
    AppendTail(Vec<HistoryCell>),
}

pub(crate) enum PreparedHistoryUpdate {
    ReplayAll(Vec<Line<'static>>),
    AppendTail(Vec<Line<'static>>),
}

pub(crate) fn init() -> Result<TerminalGuard> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let init_result = (|| -> Result<TerminalGuard> {
        execute!(stdout, EnableBracketedPaste)?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = custom_terminal::Terminal::new(backend, color_compat::ColorDepth::detect())?;
        Ok(TerminalGuard { terminal })
    })();
    if init_result.is_err() {
        let _ = restore();
    }
    init_result
}

pub(crate) fn restore() -> Result<()> {
    let _ = execute!(io::stdout(), DisableBracketedPaste);
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

    pub(crate) fn draw_projection(
        &mut self,
        projection: PreparedHistoryProjection,
        render: impl FnOnce(&mut Frame),
    ) -> Result<()> {
        stdout().sync_update(|_| {
            let mut coordinator = DrawCoordinator::new(&mut self.terminal);
            coordinator.draw_frame(projection, render)?;
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
