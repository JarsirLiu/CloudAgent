mod in_process;
mod remote;
mod stdio;

use agent_core::ServerRequestDecision;
use agent_protocol::{
    AppServerMessage, AppServerNotification, ConversationHistoryPageResponse,
    ConversationHistoryResponse, ConversationListResponse, ConversationStatusResponse,
    JsonRpcErrorPayload, JsonRpcRequest, NodeStatusResponse, NodeStopResponse,
    NotificationDelivery, OnlineNodeListResponse, PlatformConfigResponse,
    PlatformControlListResponse, PlatformControlStatusResponse, PlatformControlUpdateResponse,
    RequestId, SelectTargetNodeResponse, SessionBootstrapContext, SkillsListResponse,
    UserTurnInput, WeixinLoginStartResponse, WeixinLoginStatusResponse, classify_notification,
};
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::error::Error;
use std::fmt;
use std::io::Error as IoError;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::mpsc;

use in_process::InProcessAppServerRequestHandle;
pub use in_process::InProcessClientConfig;
use remote::RemoteAppServerRequestHandle;
pub use remote::{RemoteAppServerClient, RemoteClientConfig};
pub use stdio::{StdioAppServerClient, StdioClientConfig};

pub const DEFAULT_EVENT_CHANNEL_CAPACITY: usize = 128;
static REQUEST_ID_COUNTER: AtomicI64 = AtomicI64::new(1);

#[derive(Clone, Debug)]
pub struct AppServerConnectInfo {
    pub client_name: String,
    pub client_version: String,
    pub experimental_api: bool,
    pub opt_out_notification_methods: Vec<String>,
    pub channel_capacity: usize,
    pub session_context: Option<SessionBootstrapContext>,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum AppServerEvent {
    Message(AppServerMessage),
    Lagged { skipped: usize },
    Disconnected { message: String },
}

pub enum AppServerClient {
    InProcess(in_process::InProcessAppServerClient),
    Remote(remote::RemoteAppServerClient),
}

#[derive(Clone)]
pub enum AppServerRequestHandle {
    InProcess(InProcessAppServerRequestHandle),
    Remote(RemoteAppServerRequestHandle),
}

#[derive(Debug)]
pub enum TypedRequestError {
    Transport {
        method: String,
        source: IoError,
    },
    Server {
        method: String,
        source: JsonRpcErrorPayload,
    },
    Deserialize {
        method: String,
        source: serde_json::Error,
    },
}

impl fmt::Display for TypedRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport { method, source } => write!(f, "{method} transport error: {source}"),
            Self::Server { method, source } => write!(f, "{method} failed: {}", source.message),
            Self::Deserialize { method, source } => {
                write!(f, "{method} response decode error: {source}")
            }
        }
    }
}

impl Error for TypedRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport { source, .. } => Some(source),
            Self::Server { .. } => None,
            Self::Deserialize { source, .. } => Some(source),
        }
    }
}

impl AppServerClient {
    pub fn in_process(config: InProcessClientConfig) -> Self {
        Self::InProcess(in_process::InProcessAppServerClient::start(config))
    }

    pub async fn remote(config: RemoteClientConfig) -> Result<Self> {
        Ok(Self::Remote(
            remote::RemoteAppServerClient::connect(config).await?,
        ))
    }

    pub fn send_command(&self, command: agent_protocol::AppClientCommand) -> Result<()> {
        match self {
            Self::InProcess(client) => client.send_command(command),
            Self::Remote(client) => client.send_command(command),
        }
    }

    pub fn request_handle(&self) -> AppServerRequestHandle {
        match self {
            Self::InProcess(client) => AppServerRequestHandle::InProcess(client.request_handle()),
            Self::Remote(client) => AppServerRequestHandle::Remote(client.request_handle()),
        }
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        match self {
            Self::InProcess(client) => client.request_typed(request).await,
            Self::Remote(client) => client.request_typed(request).await,
        }
    }

    pub async fn request_conversation_list_typed(
        &self,
    ) -> Result<ConversationListResponse, TypedRequestError> {
        self.request_handle()
            .request_conversation_list_typed()
            .await
    }

    pub async fn request_skills_list_typed(&self) -> Result<SkillsListResponse, TypedRequestError> {
        self.request_handle().request_skills_list_typed().await
    }

    pub async fn request_conversation_status_typed(
        &self,
        conversation_id: impl Into<String>,
    ) -> Result<ConversationStatusResponse, TypedRequestError> {
        self.request_handle()
            .request_conversation_status_typed(conversation_id)
            .await
    }

    pub async fn request_conversation_history_typed(
        &self,
        conversation_id: impl Into<String>,
    ) -> Result<ConversationHistoryResponse, TypedRequestError> {
        self.request_handle()
            .request_conversation_history_typed(conversation_id)
            .await
    }

    pub async fn request_conversation_history_page_typed(
        &self,
        conversation_id: impl Into<String>,
        before_turn_id: Option<String>,
        limit: usize,
    ) -> Result<ConversationHistoryPageResponse, TypedRequestError> {
        self.request_handle()
            .request_conversation_history_page_typed(conversation_id, before_turn_id, limit)
            .await
    }

    pub async fn request_online_nodes_typed(
        &self,
    ) -> Result<OnlineNodeListResponse, TypedRequestError> {
        self.request_handle().request_online_nodes_typed().await
    }

    pub async fn select_target_node_typed(
        &self,
        node_id: impl Into<String>,
    ) -> Result<SelectTargetNodeResponse, TypedRequestError> {
        self.request_handle()
            .select_target_node_typed(node_id)
            .await
    }

    pub async fn request_platform_list_typed(
        &self,
    ) -> Result<PlatformControlListResponse, TypedRequestError> {
        self.request_handle().request_platform_list_typed().await
    }

    pub async fn request_node_status_typed(&self) -> Result<NodeStatusResponse, TypedRequestError> {
        self.request_handle().request_node_status_typed().await
    }

    pub async fn stop_node_typed(&self) -> Result<NodeStopResponse, TypedRequestError> {
        self.request_handle().stop_node_typed().await
    }

    pub async fn request_platform_status_typed(
        &self,
        platform: impl Into<String>,
    ) -> Result<PlatformControlStatusResponse, TypedRequestError> {
        self.request_handle()
            .request_platform_status_typed(platform)
            .await
    }

    pub async fn set_platform_enabled_typed(
        &self,
        platform: impl Into<String>,
        enabled: bool,
    ) -> Result<PlatformControlUpdateResponse, TypedRequestError> {
        self.request_handle()
            .set_platform_enabled_typed(platform, enabled)
            .await
    }

    pub async fn request_platform_config_typed(
        &self,
        platform: impl Into<String>,
    ) -> Result<PlatformConfigResponse, TypedRequestError> {
        self.request_handle()
            .request_platform_config_typed(platform)
            .await
    }

    pub async fn set_platform_config_value_typed(
        &self,
        platform: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<PlatformConfigResponse, TypedRequestError> {
        self.request_handle()
            .set_platform_config_value_typed(platform, key, value)
            .await
    }

    pub async fn clear_platform_config_value_typed(
        &self,
        platform: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<PlatformConfigResponse, TypedRequestError> {
        self.request_handle()
            .clear_platform_config_value_typed(platform, key)
            .await
    }

    pub async fn start_weixin_login_typed(
        &self,
    ) -> Result<WeixinLoginStartResponse, TypedRequestError> {
        self.request_handle().start_weixin_login_typed().await
    }

    pub async fn check_weixin_login_typed(
        &self,
        session_id: impl Into<String>,
    ) -> Result<WeixinLoginStatusResponse, TypedRequestError> {
        self.request_handle()
            .check_weixin_login_typed(session_id)
            .await
    }

    pub fn submit_turn(&self, input: UserTurnInput) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::SubmitTurn(input))
    }

    pub fn resolve_server_request(
        &self,
        conversation_id: impl Into<String>,
        request_id: agent_protocol::RequestId,
        decision: ServerRequestDecision,
    ) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::ResolveServerRequest {
            conversation_id: conversation_id.into(),
            request_id,
            decision,
        })
    }

    pub fn reject_server_request(
        &self,
        conversation_id: impl Into<String>,
        request_id: agent_protocol::RequestId,
        reason: impl Into<String>,
    ) -> Result<()> {
        self.resolve_server_request(
            conversation_id,
            request_id,
            agent_core::ServerRequestDecision::decline(Some(reason.into())),
        )
    }

    pub fn interrupt_turn(&self, conversation_id: impl Into<String>) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::InterruptTurn {
            conversation_id: conversation_id.into(),
        })
    }

    pub fn list_conversations(&self) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::ListConversations)
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
            Self::Remote(client) => client.next_event().await,
        }
    }

    pub fn try_next_event(&mut self) -> Option<AppServerEvent> {
        match self {
            Self::InProcess(client) => client.try_next_event(),
            Self::Remote(client) => client.try_next_event(),
        }
    }

    pub async fn shutdown(self) -> Result<()> {
        match self {
            Self::InProcess(client) => client.shutdown().await,
            Self::Remote(client) => client.shutdown().await,
        }
    }
}

impl AppServerRequestHandle {
    pub fn send_command(&self, command: agent_protocol::AppClientCommand) -> Result<()> {
        match self {
            Self::InProcess(handle) => handle.send_command(command),
            Self::Remote(handle) => handle.send_command(command),
        }
    }

    pub fn submit_turn(&self, input: UserTurnInput) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::SubmitTurn(input))
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        match self {
            Self::InProcess(handle) => handle.request_typed(request).await,
            Self::Remote(handle) => handle.request_typed(request).await,
        }
    }

    pub async fn request_conversation_list_typed(
        &self,
    ) -> Result<ConversationListResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "conversation/list".to_string(),
            params: None,
        })
        .await
    }

    pub async fn request_skills_list_typed(&self) -> Result<SkillsListResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "skills/list".to_string(),
            params: None,
        })
        .await
    }

    pub async fn request_conversation_status_typed(
        &self,
        conversation_id: impl Into<String>,
    ) -> Result<ConversationStatusResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "conversation/status".to_string(),
            params: Some(serde_json::json!({
                "conversation_id": conversation_id.into(),
            })),
        })
        .await
    }

    pub async fn request_conversation_history_typed(
        &self,
        conversation_id: impl Into<String>,
    ) -> Result<ConversationHistoryResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "conversation/history".to_string(),
            params: Some(serde_json::json!({
                "conversation_id": conversation_id.into(),
            })),
        })
        .await
    }

    pub async fn request_conversation_history_page_typed(
        &self,
        conversation_id: impl Into<String>,
        before_turn_id: Option<String>,
        limit: usize,
    ) -> Result<ConversationHistoryPageResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "conversation/historyPage".to_string(),
            params: Some(serde_json::json!({
                "conversation_id": conversation_id.into(),
                "before_turn_id": before_turn_id,
                "limit": limit,
            })),
        })
        .await
    }

    pub async fn request_online_nodes_typed(
        &self,
    ) -> Result<OnlineNodeListResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "hub/node/list".to_string(),
            params: None,
        })
        .await
    }

    pub async fn select_target_node_typed(
        &self,
        node_id: impl Into<String>,
    ) -> Result<SelectTargetNodeResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "hub/node/select".to_string(),
            params: Some(serde_json::json!({
                "node_id": node_id.into(),
            })),
        })
        .await
    }

    pub async fn request_platform_list_typed(
        &self,
    ) -> Result<PlatformControlListResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "platform/list".to_string(),
            params: None,
        })
        .await
    }

    pub async fn request_node_status_typed(&self) -> Result<NodeStatusResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "node/status".to_string(),
            params: None,
        })
        .await
    }

    pub async fn stop_node_typed(&self) -> Result<NodeStopResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "node/stop".to_string(),
            params: None,
        })
        .await
    }

    pub async fn request_platform_status_typed(
        &self,
        platform: impl Into<String>,
    ) -> Result<PlatformControlStatusResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "platform/status".to_string(),
            params: Some(serde_json::json!({
                "platform": platform.into(),
            })),
        })
        .await
    }

    pub async fn set_platform_enabled_typed(
        &self,
        platform: impl Into<String>,
        enabled: bool,
    ) -> Result<PlatformControlUpdateResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "platform/setEnabled".to_string(),
            params: Some(serde_json::json!({
                "platform": platform.into(),
                "enabled": enabled,
            })),
        })
        .await
    }

    pub async fn request_platform_config_typed(
        &self,
        platform: impl Into<String>,
    ) -> Result<PlatformConfigResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "platform/config".to_string(),
            params: Some(serde_json::json!({
                "platform": platform.into(),
            })),
        })
        .await
    }

    pub async fn set_platform_config_value_typed(
        &self,
        platform: impl Into<String>,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<PlatformConfigResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "platform/config/set".to_string(),
            params: Some(serde_json::json!({
                "platform": platform.into(),
                "key": key.into(),
                "value": value.into(),
            })),
        })
        .await
    }

    pub async fn clear_platform_config_value_typed(
        &self,
        platform: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<PlatformConfigResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "platform/config/clear".to_string(),
            params: Some(serde_json::json!({
                "platform": platform.into(),
                "key": key.into(),
            })),
        })
        .await
    }

    pub async fn start_weixin_login_typed(
        &self,
    ) -> Result<WeixinLoginStartResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "weixin/login/start".to_string(),
            params: None,
        })
        .await
    }

    pub async fn check_weixin_login_typed(
        &self,
        session_id: impl Into<String>,
    ) -> Result<WeixinLoginStatusResponse, TypedRequestError> {
        self.request_typed(JsonRpcRequest {
            id: next_request_id(),
            method: "weixin/login/check".to_string(),
            params: Some(serde_json::json!({
                "session_id": session_id.into(),
            })),
        })
        .await
    }

    pub fn resolve_server_request(
        &self,
        conversation_id: impl Into<String>,
        request_id: agent_protocol::RequestId,
        decision: ServerRequestDecision,
    ) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::ResolveServerRequest {
            conversation_id: conversation_id.into(),
            request_id,
            decision,
        })
    }

    pub fn reject_server_request(
        &self,
        conversation_id: impl Into<String>,
        request_id: agent_protocol::RequestId,
        reason: impl Into<String>,
    ) -> Result<()> {
        self.resolve_server_request(
            conversation_id,
            request_id,
            agent_core::ServerRequestDecision::decline(Some(reason.into())),
        )
    }

    pub fn interrupt_turn(&self, conversation_id: impl Into<String>) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::InterruptTurn {
            conversation_id: conversation_id.into(),
        })
    }

    pub fn list_conversations(&self) -> Result<()> {
        self.send_command(agent_protocol::AppClientCommand::ListConversations)
    }

    pub fn request_conversation_history(&self, conversation_id: impl Into<String>) -> Result<()> {
        self.send_command(
            agent_protocol::AppClientCommand::RequestConversationHistory {
                conversation_id: conversation_id.into(),
            },
        )
    }
}

fn next_request_id() -> RequestId {
    RequestId::Integer(REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
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
        CommandApprovalRequest, CommandExecutionStatus, ServerRequest, TranscriptItem,
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
                call_id: Some("call-1".to_string()),
                item: TranscriptItem::CommandExecution {
                    id: "tool:1".to_string(),
                    tool_name: "exec_command".to_string(),
                    command: "pwd".to_string(),
                    current_directory: String::new(),
                    status: agent_core::CommandExecutionStatus::InProgress,
                    exit_code: None,
                    output: Some(String::new()),
                    duration_ms: None,
                    summary: String::new(),
                },
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
                    output: Some("D:\\work".to_string()),
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
