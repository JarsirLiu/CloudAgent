mod color_compat;
pub mod custom_terminal;
mod draw_coordinator;
pub mod events;
pub(crate) mod history_flush_queue;
mod insert_history;
mod keyboard_modes;
mod resize_reflow_cap;

use anyhow::Result;
use crossterm::SynchronizedUpdate;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use std::io::{self, stdout};
use std::panic;
use std::sync::Once;

use crate::ui::widgets::history_cell::HistoryCell;

use color_compat::TerminalCapabilities;
pub use color_compat::apply_color_cli_preference;
pub(crate) use custom_terminal::Frame;
use draw_coordinator::DrawCoordinator;
pub(crate) use events::{FrameRequester, UiEvent, spawn_tui_event_loop};
use history_flush_queue::HistoryFlushQueue;
pub(crate) use insert_history::{
    insert_history_lines_raw, prepare_history_lines, prepare_history_tail_lines,
    repaint_history_tail_raw,
};
pub(crate) use resize_reflow_cap::resize_reflow_max_rows;

static INSTALL_PANIC_HOOK: Once = Once::new();

pub(crate) struct TerminalGuard {
    pub(crate) terminal: custom_terminal::Terminal<CrosstermBackend<io::Stdout>>,
    capabilities: TerminalCapabilities,
    history_flush_queue: HistoryFlushQueue,
}

pub(crate) struct HistoryProjection {
    pub(crate) viewport_height: u16,
    pub(crate) history_render_metrics: HistoryRenderMetrics,
    pub(crate) history_update: HistoryUpdate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HistoryRenderMetrics {
    pub(crate) width: usize,
    pub(crate) left_padding: usize,
}

pub(crate) struct PreparedHistoryProjection {
    pub(crate) viewport_height: u16,
    pub(crate) history_update: PreparedHistoryUpdate,
}

pub(crate) enum HistoryUpdate {
    ReplayAll(Vec<HistoryCell>),
    AppendTail(Vec<HistoryCell>),
    ReflowVisibleTail {
        cells: Vec<HistoryCell>,
        max_rows: usize,
    },
}

pub(crate) enum PreparedHistoryUpdate {
    ReplayAll {
        cells: Vec<HistoryCell>,
        render_metrics: HistoryRenderMetrics,
        max_rows: Option<usize>,
    },
    AppendTail {
        cells: Vec<HistoryCell>,
        render_metrics: HistoryRenderMetrics,
    },
    ReflowVisibleTail {
        cells: Vec<HistoryCell>,
        render_metrics: HistoryRenderMetrics,
        max_rows: usize,
    },
}

pub(crate) fn init() -> Result<TerminalGuard> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let init_result = (|| -> Result<TerminalGuard> {
        execute!(stdout, EnableBracketedPaste)?;
        keyboard_modes::enable_keyboard_enhancement();
        let backend = CrosstermBackend::new(io::stdout());
        let capabilities = TerminalCapabilities::detect();
        let terminal = custom_terminal::Terminal::new(backend, capabilities)?;
        Ok(TerminalGuard {
            terminal,
            capabilities,
            history_flush_queue: HistoryFlushQueue::default(),
        })
    })();
    if init_result.is_err() {
        let _ = restore();
    }
    init_result
}

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
                let mut coordinator =
                    DrawCoordinator::new(&mut self.terminal, &mut self.history_flush_queue);
                coordinator.draw_frame(projection, render)?;
                Ok::<(), anyhow::Error>(())
            })??;
        } else {
            let mut coordinator =
                DrawCoordinator::new(&mut self.terminal, &mut self.history_flush_queue);
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
