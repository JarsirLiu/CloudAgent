mod in_process;
mod stdio;

use agent_protocol::{
    AppServerMessage, AppServerNotification, NotificationDelivery, classify_notification,
};
use anyhow::Result;
use tokio::sync::mpsc;

pub use in_process::InProcessClientConfig;
pub use stdio::StdioClientConfig;

pub const DEFAULT_EVENT_CHANNEL_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
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

    pub fn request_conversation_history(&self, conversation_id: impl Into<String>) -> Result<()> {
        self.send_command(
            agent_protocol::AppClientCommand::RequestConversationHistory {
                conversation_id: conversation_id.into(),
            },
        )
    }

    pub fn request_conversation_history_page(
        &self,
        conversation_id: impl Into<String>,
        before_turn_id: Option<String>,
        limit: usize,
    ) -> Result<()> {
        self.send_command(
            agent_protocol::AppClientCommand::RequestConversationHistoryPage {
                conversation_id: conversation_id.into(),
                before_turn_id,
                limit,
            },
        )
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        match self {
            Self::InProcess(client) => client.next_event().await,
            Self::Stdio(client) => client.next_event().await,
        }
    }

    pub fn try_next_event(&mut self) -> Option<AppServerEvent> {
        match self {
            Self::InProcess(client) => client.try_next_event(),
            Self::Stdio(client) => client.try_next_event(),
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
        AppServerMessage::Notification(notification) => {
            notification_requires_delivery(notification)
        }
    }
}

fn notification_requires_delivery(notification: &AppServerNotification) -> bool {
    matches!(
        classify_notification(notification),
        (_, NotificationDelivery::Lossless)
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        CommandApprovalRequest, CommandExecutionStatus, ServerRequest, TranscriptItem, TurnItemKind,
    };
    use agent_protocol::{AppServerNotification, RequestId};

    fn info_event(message: &str) -> AppServerEvent {
        AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::Info {
                conversation_id: "default".to_string(),
                message: message.to_string(),
            },
        ))
    }

    fn text_delta_event(delta: &str) -> AppServerEvent {
        AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::AgentMessageDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "assistant:1".to_string(),
                delta: delta.to_string(),
            },
        ))
    }

    fn command_output_event(delta: &str) -> AppServerEvent {
        AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::CommandExecutionOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "tool:1".to_string(),
                call_id: Some("call-1".to_string()),
                delta: delta.to_string(),
            },
        ))
    }

    fn item_started_event() -> AppServerEvent {
        AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::ItemStarted {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "tool:1".to_string(),
                call_id: Some("call-1".to_string()),
                kind: TurnItemKind::CommandExecution,
                title: Some("pwd".to_string()),
            },
        ))
    }

    fn item_completed_event() -> AppServerEvent {
        AppServerEvent::Message(AppServerMessage::Notification(
            AppServerNotification::ItemCompleted {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                call_id: Some("call-1".to_string()),
                item: TranscriptItem::CommandExecution {
                    id: "tool:1".to_string(),
                    tool_name: "exec_command".to_string(),
                    command: "pwd".to_string(),
                    current_directory: "D:\\work".to_string(),
                    status: CommandExecutionStatus::Completed,
                    exit_code: Some(0),
                    stdout: Some("D:\\work".to_string()),
                    stderr: Some(String::new()),
                    aggregated_output: Some("D:\\work".to_string()),
                    duration_ms: Some(1),
                    summary: "current directory is D:\\work".to_string(),
                },
            },
        ))
    }

    fn server_request_event() -> AppServerEvent {
        AppServerEvent::Message(AppServerMessage::Request(
            agent_protocol::AppServerRequest::ServerRequest {
                request_id: RequestId::Integer(1),
                conversation_id: "default".to_string(),
                request: ServerRequest::CommandApproval {
                    request: CommandApprovalRequest {
                        turn_id: "turn-1".to_string(),
                        tool_call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        reason: "need approval".to_string(),
                        command_preview: "{\"command\":\"pwd\"}".to_string(),
                    },
                },
            },
        ))
    }

    #[tokio::test]
    async fn non_critical_events_drop_when_queue_is_full() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(info_event("already queued"))
            .await
            .expect("seed queue");
        let mut skipped = 0usize;

        assert!(forward_event(&tx, &mut skipped, info_event("drop me")).await);
        assert_eq!(skipped, 1);

        let first = rx.recv().await.expect("seed event");
        match first {
            AppServerEvent::Message(AppServerMessage::Notification(
                AppServerNotification::Info { message, .. },
            )) => assert_eq!(message, "already queued"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn lossless_events_flush_lag_marker_before_delivery() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(info_event("already queued"))
            .await
            .expect("seed queue");
        let mut skipped = 0usize;

        assert!(forward_event(&tx, &mut skipped, info_event("drop me")).await);
        assert_eq!(skipped, 1);

        let sender = tokio::spawn(async move {
            let mut skipped = skipped;
            let delivered = forward_event(&tx, &mut skipped, item_completed_event()).await;
            (delivered, skipped)
        });

        let first = rx.recv().await.expect("first event");
        let second = rx.recv().await.expect("second event");
        let third = rx.recv().await.expect("third event");
        let (delivered, skipped) = sender.await.expect("sender task");
        assert!(delivered);
        assert_eq!(skipped, 0);

        match first {
            AppServerEvent::Message(AppServerMessage::Notification(
                AppServerNotification::Info { message, .. },
            )) => assert_eq!(message, "already queued"),
            other => panic!("unexpected first event: {other:?}"),
        }
        match second {
            AppServerEvent::Lagged { skipped } => assert_eq!(skipped, 1),
            other => panic!("unexpected second event: {other:?}"),
        }
        match third {
            AppServerEvent::Message(AppServerMessage::Notification(
                AppServerNotification::ItemCompleted { .. },
            )) => {}
            other => panic!("unexpected third event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_and_transcript_events_are_classified_lossless() {
        assert!(event_requires_delivery(&item_started_event()));
        assert!(event_requires_delivery(&item_completed_event()));
        assert!(event_requires_delivery(&text_delta_event("hello")));
        assert!(event_requires_delivery(&server_request_event()));
        assert!(!event_requires_delivery(&command_output_event("D:\\work")));
        assert!(!event_requires_delivery(&info_event("cosmetic")));
        assert!(!event_requires_delivery(&AppServerEvent::Lagged {
            skipped: 1
        }));
        assert!(!event_requires_delivery(&AppServerEvent::Disconnected {
            message: "bye".to_string()
        }));
    }
}
