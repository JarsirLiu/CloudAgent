pub mod client;

use agent_protocol::{AppClientCommand, AppServerMessage};
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

pub trait AppServerPort {
    fn send_command(&self, command: AppClientCommand) -> Result<()>;
    fn next_message<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Option<AppServerMessage>> + 'a>>;
    fn shutdown(self) -> Pin<Box<dyn Future<Output = Result<()>>>> where Self: Sized;
}

