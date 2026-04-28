mod in_process;
pub mod transport;

use agent_protocol::{AppClientCommandEnvelope, AppServerMessageEnvelope};
use agent_runtime::AgentRuntime;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

pub use in_process::{
    InProcessClientHandle, InProcessClientSender, InProcessServer, start_in_process,
};

pub async fn run_stdio_server(
    runtime: Arc<AgentRuntime>,
    session_id: String,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()> {
    let mut client = start_in_process(runtime, session_id, auto_approve, auto_approve_reason);
    let sender = client.sender();
    let (command_tx, mut command_rx) = mpsc::unbounded_channel::<AppClientCommandEnvelope>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AppServerMessageEnvelope>();

    let read_task = tokio::spawn(async move { transport::stdio::read_commands(command_tx).await });
    let write_task = tokio::spawn(async move { transport::stdio::write_events(event_rx).await });
    let forward_events = tokio::spawn(async move {
        while let Some(message) = client.next_message().await {
            if event_tx.send(AppServerMessageEnvelope { message }).is_err() {
                break;
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    let forward_commands = tokio::spawn(async move {
        while let Some(envelope) = command_rx.recv().await {
            sender.send_command(envelope.command)?;
        }
        Ok::<(), anyhow::Error>(())
    });

    read_task.await??;
    forward_commands.await??;
    forward_events.await??;
    write_task.await??;
    Ok(())
}
