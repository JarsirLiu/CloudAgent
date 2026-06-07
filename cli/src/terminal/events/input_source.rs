use std::time::Duration;

use crossterm::event::{self, Event as CEvent};

pub(super) trait TerminalInputSource {
    fn poll(&mut self, timeout: Duration) -> std::io::Result<bool>;
    fn read(&mut self) -> std::io::Result<CEvent>;
}

#[derive(Default)]
pub(super) struct CrosstermInputSource;

impl TerminalInputSource for CrosstermInputSource {
    fn poll(&mut self, timeout: Duration) -> std::io::Result<bool> {
        event::poll(timeout)
    }

    fn read(&mut self) -> std::io::Result<CEvent> {
        event::read()
    }
}
