pub mod client;

use agent_app_server_client::AppServerEvent;
use agent_protocol::AppClientCommand;
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

pub trait AppServerPort {
    fn send_command(&self, command: AppClientCommand) -> Result<()>;
    fn next_event<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Option<AppServerEvent>> + 'a>>;
    fn shutdown(self) -> Pin<Box<dyn Future<Output = Result<()>>>> where Self: Sized;
}
