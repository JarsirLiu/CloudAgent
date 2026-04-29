mod in_process;
mod stdio;

use agent_protocol::{AppServerMessage, AppServerNotification, TurnItemDeltaKind};
use anyhow::Result;
use tokio::sync::mpsc;

pub use in_process::InProcessClientConfig;
pub use stdio::StdioClientConfig;

pub const DEFAULT_EVENT_CHANNEL_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
pub enum AppServerEvent {
    Message(AppServerMessage),
    Lagged { skipped: usize },
    Disconnected { message: String },
}

pub enum AppServerClient {
    InProcess(in_process::InProcessAppServerClient),
    Stdio(stdio::StdioAppServerClient),
}

impl AppServerClient {
    pub fn in_process(config: InProcessClientConfig) -> Self {
        Self::InProcess(in_process::InProcessAppServerClient::start(config))
    }

    pub async fn stdio(config: StdioClientConfig) -> Result<Self> {
        Ok(Self::Stdio(
            stdio::StdioAppServerClient::spawn(config).await?,
        ))
    }

    pub fn send_command(&self, command: agent_protocol::AppClientCommand) -> Result<()> {
        match self {
            Self::InProcess(client) => client.send_command(command),
            Self::Stdio(client) => client.send_command(command),
        }
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        match self {
            Self::InProcess(client) => client.next_event().await,
            Self::Stdio(client) => client.next_event().await,
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        match self {
            Self::InProcess(client) => client.shutdown().await,
            Self::Stdio(client) => client.shutdown().await,
        }
    }
}

pub(crate) fn event_requires_delivery(event: &AppServerEvent) -> bool {
    match event {
        AppServerEvent::Message(message) => app_server_message_requires_delivery(message),
        AppServerEvent::Lagged { .. } | AppServerEvent::Disconnected { .. } => false,
    }
}

fn app_server_message_requires_delivery(message: &AppServerMessage) -> bool {
    match message {
        AppServerMessage::Request(_) => true,
        AppServerMessage::Notification(notification) => matches!(
            notification,
            AppServerNotification::ItemStarted { .. }
                | AppServerNotification::ServerRequestRequested { .. }
                | AppServerNotification::ServerRequestResolved { .. }
                | AppServerNotification::ItemCompleted { .. }
                | AppServerNotification::TurnCompleted { .. }
                | AppServerNotification::TurnFailed { .. }
                | AppServerNotification::TurnCancelled { .. }
        ) || matches!(
            notification,
            AppServerNotification::ItemDelta { kind: TurnItemDeltaKind::Text, .. }
                | AppServerNotification::ItemDelta { kind: TurnItemDeltaKind::ReasoningSummary, .. }
                | AppServerNotification::ItemDelta { kind: TurnItemDeltaKind::ReasoningText, .. }
        ),
    }
}

pub(crate) async fn forward_event(
    event_tx: &mpsc::Sender<AppServerEvent>,
    skipped_events: &mut usize,
    event: AppServerEvent,
) -> bool {
    if *skipped_events > 0 {
        let lagged = AppServerEvent::Lagged {
            skipped: *skipped_events,
        };
        if event_requires_delivery(&event) {
            if event_tx.send(lagged).await.is_err() {
                return false;
            }
            *skipped_events = 0;
        } else {
            match event_tx.try_send(lagged) {
                Ok(()) => {
                    *skipped_events = 0;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    *skipped_events = skipped_events.saturating_add(1);
                    return true;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return false,
            }
        }
    }

    if event_requires_delivery(&event) {
        return event_tx.send(event).await.is_ok();
    }

    match event_tx.try_send(event) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            *skipped_events = skipped_events.saturating_add(1);
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}
