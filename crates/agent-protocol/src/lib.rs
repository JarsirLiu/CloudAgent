mod jsonrpc;

pub use agent_core::{
    CommandExecutionStatus, ConversationTurn, EventMsg, ServerRequest, ServerRequestDecision,
    StructuredToolResult, ToolApprovalRequest, ToolCall, ToolResult, ToolSpec, TranscriptItem,
    TurnId, TurnItemDeltaKind, TurnItemKind, TurnState, WriteFileStatus,
};
pub use jsonrpc::{
    JsonRpcError, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FrontendMode {
    Idle,
    Running,
    WaitingForServerRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserTurnInput {
    pub conversation_id: String,
    pub content: String,
}

pub type NotificationSequence = u64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SequencedAppServerMessage {
    pub sequence: NotificationSequence,
    pub message: AppServerMessage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConversationStatus {
    Idle,
    Busy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationSnapshot {
    pub conversation_id: String,
    pub conversation_status: ConversationStatus,
    pub active_turn: Option<TurnId>,
    pub turn_state: Option<TurnState>,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppClientCommand {
    SubmitTurn(UserTurnInput),
    ResolveServerRequest {
        conversation_id: String,
        request_id: RequestId,
        approved: bool,
        reason: Option<String>,
    },
    InterruptTurn {
        conversation_id: String,
    },
    ResetConversation {
        conversation_id: String,
    },
    RequestConversationStatus {
        conversation_id: String,
    },
    RequestConversationHistory {
        conversation_id: String,
    },
    RequestConversationNotifications {
        conversation_id: String,
        after_sequence: NotificationSequence,
    },
    SubscribeConversation {
        conversation_id: String,
    },
    UnsubscribeConversation {
        conversation_id: String,
    },
    Exit,
}

impl AppClientCommand {
    pub fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::SubmitTurn(input) => Some(&input.conversation_id),
            Self::ResolveServerRequest {
                conversation_id, ..
            }
            | Self::InterruptTurn { conversation_id }
            | Self::ResetConversation { conversation_id }
            | Self::RequestConversationStatus { conversation_id }
            | Self::RequestConversationHistory { conversation_id }
            | Self::RequestConversationNotifications {
                conversation_id, ..
            }
            | Self::SubscribeConversation { conversation_id }
            | Self::UnsubscribeConversation { conversation_id } => Some(conversation_id),
            Self::Exit => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerNotification {
    FrontendStateChanged {
        conversation_id: String,
        mode: FrontendMode,
    },
    TurnStarted {
        conversation_id: String,
        turn_id: TurnId,
    },
    ItemStarted {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
    },
    AgentMessageDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    PlanDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningSummaryTextDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningTextDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    CommandExecutionOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ToolOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    FileChangeOutputDelta {
        conversation_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ItemCompleted {
        conversation_id: String,
        turn_id: TurnId,
        item: TranscriptItem,
    },
    ServerRequestRequested {
        conversation_id: String,
        turn_id: TurnId,
        request: ServerRequest,
    },
    ServerRequestResolved {
        conversation_id: String,
        turn_id: TurnId,
        request_id: RequestId,
        request: ServerRequest,
        decision: ServerRequestDecision,
    },
    TurnCompleted {
        conversation_id: String,
        turn_id: TurnId,
    },
    TurnFailed {
        conversation_id: String,
        turn_id: TurnId,
        error: String,
    },
    TurnCancelled {
        conversation_id: String,
        turn_id: TurnId,
        reason: String,
    },
    ConversationStatus {
        conversation_id: String,
        snapshot: ConversationSnapshot,
    },
    ConversationHistory {
        conversation_id: String,
        turns: Vec<ConversationTurn>,
    },
    ConversationNotifications {
        conversation_id: String,
        from_sequence: NotificationSequence,
        messages: Vec<SequencedAppServerMessage>,
    },
    ConversationSubscriptionChanged {
        conversation_id: String,
        subscribed: bool,
    },
    Info {
        conversation_id: String,
        message: String,
    },
    Error {
        conversation_id: String,
        message: String,
    },
}

impl AppServerNotification {
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::FrontendStateChanged {
                conversation_id, ..
            }
            | Self::TurnStarted {
                conversation_id, ..
            }
            | Self::ItemStarted {
                conversation_id, ..
            }
            | Self::AgentMessageDelta {
                conversation_id, ..
            }
            | Self::PlanDelta {
                conversation_id, ..
            }
            | Self::ReasoningSummaryTextDelta {
                conversation_id, ..
            }
            | Self::ReasoningTextDelta {
                conversation_id, ..
            }
            | Self::CommandExecutionOutputDelta {
                conversation_id, ..
            }
            | Self::ToolOutputDelta {
                conversation_id, ..
            }
            | Self::FileChangeOutputDelta {
                conversation_id, ..
            }
            | Self::ItemCompleted {
                conversation_id, ..
            }
            | Self::ServerRequestRequested {
                conversation_id, ..
            }
            | Self::ServerRequestResolved {
                conversation_id, ..
            }
            | Self::TurnCompleted {
                conversation_id, ..
            }
            | Self::TurnFailed {
                conversation_id, ..
            }
            | Self::TurnCancelled {
                conversation_id, ..
            }
            | Self::ConversationStatus {
                conversation_id, ..
            }
            | Self::ConversationHistory {
                conversation_id, ..
            }
            | Self::ConversationNotifications {
                conversation_id, ..
            }
            | Self::ConversationSubscriptionChanged {
                conversation_id, ..
            }
            | Self::Info {
                conversation_id, ..
            }
            | Self::Error {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerRequest {
    ServerRequest {
        request_id: RequestId,
        conversation_id: String,
        request: ServerRequest,
    },
}

impl AppServerRequest {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::ServerRequest { request_id, .. } => request_id,
        }
    }

    pub fn conversation_id(&self) -> &str {
        match self {
            Self::ServerRequest {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppServerMessage {
    Notification(AppServerNotification),
    Request(AppServerRequest),
}

impl AppServerMessage {
    pub fn conversation_id(&self) -> Option<&str> {
        match self {
            Self::Notification(notification) => Some(notification.conversation_id()),
            Self::Request(request) => Some(request.conversation_id()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationDelivery {
    Lossless,
    BestEffort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationStream {
    CoreTranscript,
    Control,
    Diagnostic,
}

pub fn classify_notification(
    notification: &AppServerNotification,
) -> (NotificationStream, NotificationDelivery) {
    match notification {
        AppServerNotification::AgentMessageDelta { .. }
        | AppServerNotification::PlanDelta { .. }
        | AppServerNotification::ReasoningSummaryTextDelta { .. }
        | AppServerNotification::ReasoningTextDelta { .. }
        | AppServerNotification::ItemCompleted { .. }
        | AppServerNotification::TurnCompleted { .. } => (
            NotificationStream::CoreTranscript,
            NotificationDelivery::Lossless,
        ),
        AppServerNotification::TurnStarted { .. }
        | AppServerNotification::ItemStarted { .. }
        | AppServerNotification::ServerRequestRequested { .. }
        | AppServerNotification::ServerRequestResolved { .. }
        | AppServerNotification::TurnFailed { .. }
        | AppServerNotification::TurnCancelled { .. }
        | AppServerNotification::ConversationStatus { .. }
        | AppServerNotification::ConversationHistory { .. }
        | AppServerNotification::ConversationNotifications { .. }
        | AppServerNotification::ConversationSubscriptionChanged { .. }
        | AppServerNotification::FrontendStateChanged { .. } => {
            (NotificationStream::Control, NotificationDelivery::Lossless)
        }
        AppServerNotification::CommandExecutionOutputDelta { .. }
        | AppServerNotification::ToolOutputDelta { .. }
        | AppServerNotification::FileChangeOutputDelta { .. } => (
            NotificationStream::Control,
            NotificationDelivery::BestEffort,
        ),
        AppServerNotification::Info { .. } | AppServerNotification::Error { .. } => (
            NotificationStream::Diagnostic,
            NotificationDelivery::BestEffort,
        ),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppClientCommandEnvelope {
    pub request_id: RequestId,
    pub command: AppClientCommand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppServerMessageEnvelope {
    pub message: AppServerMessage,
}

impl TryFrom<JsonRpcMessage> for AppClientCommandEnvelope {
    type Error = anyhow::Error;

    fn try_from(message: JsonRpcMessage) -> Result<Self, Self::Error> {
        match message {
            JsonRpcMessage::Request(request) => command_from_request(request),
            JsonRpcMessage::Notification(notification) => command_from_notification(notification),
            JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {
                anyhow::bail!("client envelope expects a request or notification")
            }
        }
    }
}

impl From<AppClientCommandEnvelope> for JsonRpcMessage {
    fn from(envelope: AppClientCommandEnvelope) -> Self {
        let (method, params) = command_method_and_params(&envelope.command);
        JsonRpcMessage::Request(JsonRpcRequest {
            id: envelope.request_id,
            method: method.to_string(),
            params: Some(params),
        })
    }
}

impl From<AppServerMessageEnvelope> for JsonRpcMessage {
    fn from(envelope: AppServerMessageEnvelope) -> Self {
        match envelope.message {
            AppServerMessage::Notification(notification) => {
                let (method, params) = notification_method_and_params(&notification);
                JsonRpcMessage::Notification(JsonRpcNotification {
                    method: method.to_string(),
                    params: Some(params),
                })
            }
            AppServerMessage::Request(request) => {
                let (id, method, params) = request_method_and_params(&request);
                JsonRpcMessage::Request(JsonRpcRequest {
                    id,
                    method: method.to_string(),
                    params: Some(params),
                })
            }
        }
    }
}

impl TryFrom<JsonRpcMessage> for AppServerMessageEnvelope {
    type Error = anyhow::Error;

    fn try_from(message: JsonRpcMessage) -> Result<Self, Self::Error> {
        match message {
            JsonRpcMessage::Notification(notification) => Ok(AppServerMessageEnvelope {
                message: AppServerMessage::Notification(parse_server_notification(
                    &notification.method,
                    notification.params,
                )?),
            }),
            JsonRpcMessage::Request(request) => Ok(AppServerMessageEnvelope {
                message: AppServerMessage::Request(parse_server_request(
                    request.id,
                    &request.method,
                    request.params,
                )?),
            }),
            JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {
                anyhow::bail!("server envelope expects a notification or request")
            }
        }
    }
}

fn command_from_request(request: JsonRpcRequest) -> anyhow::Result<AppClientCommandEnvelope> {
    let command = parse_command(&request.method, request.params)?;
    Ok(AppClientCommandEnvelope {
        request_id: request.id,
        command,
    })
}

fn command_from_notification(
    notification: JsonRpcNotification,
) -> anyhow::Result<AppClientCommandEnvelope> {
    let command = parse_command(&notification.method, notification.params)?;
    Ok(AppClientCommandEnvelope {
        request_id: RequestId::String("notification".to_string()),
        command,
    })
}

fn parse_command(method: &str, params: Option<Value>) -> anyhow::Result<AppClientCommand> {
    let params = params.unwrap_or(Value::Null);
    match method {
        "turn/start" => Ok(AppClientCommand::SubmitTurn(serde_json::from_value(
            params,
        )?)),
        "turn/interrupt" => Ok(AppClientCommand::InterruptTurn {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/reset" => Ok(AppClientCommand::ResetConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/status" => Ok(AppClientCommand::RequestConversationStatus {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/history" => Ok(AppClientCommand::RequestConversationHistory {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/notifications" => Ok(AppClientCommand::RequestConversationNotifications {
            conversation_id: value_field(params.clone(), "conversation_id")?,
            after_sequence: value_field(params, "after_sequence")?,
        }),
        "conversation/subscribe" => Ok(AppClientCommand::SubscribeConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/unsubscribe" => Ok(AppClientCommand::UnsubscribeConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "serverRequest/resolve" => Ok(AppClientCommand::ResolveServerRequest {
            conversation_id: value_field(params.clone(), "conversation_id")?,
            request_id: value_field(params.clone(), "request_id")?,
            approved: value_field(params.clone(), "approved")?,
            reason: value_field(params, "reason")?,
        }),
        "app/exit" => Ok(AppClientCommand::Exit),
        other => anyhow::bail!("unsupported request method: {other}"),
    }
}

fn command_method_and_params(command: &AppClientCommand) -> (&'static str, Value) {
    match command {
        AppClientCommand::SubmitTurn(input) => (
            "turn/start",
            serde_json::to_value(input).unwrap_or(Value::Null),
        ),
        AppClientCommand::ResolveServerRequest { .. } => (
            "serverRequest/resolve",
            serde_json::to_value(command).unwrap_or(Value::Null),
        ),
        AppClientCommand::InterruptTurn { conversation_id } => (
            "turn/interrupt",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::ResetConversation { conversation_id } => (
            "conversation/reset",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationStatus { conversation_id } => (
            "conversation/status",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationHistory { conversation_id } => (
            "conversation/history",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationNotifications {
            conversation_id,
            after_sequence,
        } => (
            "conversation/notifications",
            serde_json::json!({
                "conversation_id": conversation_id,
                "after_sequence": after_sequence
            }),
        ),
        AppClientCommand::SubscribeConversation { conversation_id } => (
            "conversation/subscribe",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::UnsubscribeConversation { conversation_id } => (
            "conversation/unsubscribe",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::Exit => ("app/exit", Value::Null),
    }
}

fn notification_method_and_params(notification: &AppServerNotification) -> (&'static str, Value) {
    match notification {
        AppServerNotification::FrontendStateChanged { .. } => (
            "frontend/stateChanged",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::TurnStarted { .. } => (
            "turn/started",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ItemStarted { .. } => (
            "item/started",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::AgentMessageDelta { .. } => (
            "item/agentMessage/delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::PlanDelta { .. } => (
            "item/plan/delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ReasoningSummaryTextDelta { .. } => (
            "item/reasoning/summaryTextDelta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ReasoningTextDelta { .. } => (
            "item/reasoning/textDelta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::CommandExecutionOutputDelta { .. } => (
            "item/commandExecution/outputDelta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ToolOutputDelta { .. } => (
            "item/tool/outputDelta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::FileChangeOutputDelta { .. } => (
            "item/fileChange/outputDelta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ItemCompleted { .. } => (
            "item/completed",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ServerRequestRequested { .. } => (
            "serverRequest/requested",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ServerRequestResolved { .. } => (
            "serverRequest/resolved",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::TurnCompleted { .. } => (
            "turn/completed",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::TurnFailed { .. } => (
            "turn/failed",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::TurnCancelled { .. } => (
            "turn/cancelled",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationStatus { .. } => (
            "conversation/status",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationHistory { .. } => (
            "conversation/history",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationNotifications { .. } => (
            "conversation/notifications",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationSubscriptionChanged { .. } => (
            "conversation/subscriptionChanged",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::Info { .. } => (
            "app/info",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::Error { .. } => (
            "app/error",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
    }
}

fn request_method_and_params(request: &AppServerRequest) -> (RequestId, &'static str, Value) {
    match request {
        AppServerRequest::ServerRequest {
            request_id,
            conversation_id,
            request,
        } => match request {
            ServerRequest::ToolApproval { .. } => (
                request_id.clone(),
                "serverRequest/toolApproval",
                serde_json::json!({
                    "conversation_id": conversation_id,
                    "request": request,
                }),
            ),
        },
    }
}

fn parse_server_notification(
    method: &str,
    params: Option<Value>,
) -> anyhow::Result<AppServerNotification> {
    let params = params.unwrap_or(Value::Null);
    match method {
        "frontend/stateChanged"
        | "turn/started"
        | "item/started"
        | "item/agentMessage/delta"
        | "item/plan/delta"
        | "item/reasoning/summaryTextDelta"
        | "item/reasoning/textDelta"
        | "item/commandExecution/outputDelta"
        | "item/tool/outputDelta"
        | "item/fileChange/outputDelta"
        | "item/jsonPatch/delta"
        | "item/completed"
        | "serverRequest/requested"
        | "serverRequest/resolved"
        | "turn/completed"
        | "turn/failed"
        | "turn/cancelled"
        | "conversation/status"
        | "conversation/history"
        | "conversation/notifications"
        | "conversation/subscriptionChanged"
        | "app/info"
        | "app/error" => Ok(serde_json::from_value(params)?),
        other => anyhow::bail!("unsupported notification method: {other}"),
    }
}

fn parse_server_request(
    request_id: RequestId,
    method: &str,
    params: Option<Value>,
) -> anyhow::Result<AppServerRequest> {
    let params = params.unwrap_or(Value::Null);
    match method {
        "serverRequest/toolApproval" => {
            let object = params
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("expected object params"))?;
            let conversation_id = object
                .get("conversation_id")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing `conversation_id` field"))?;
            let request = object
                .get("request")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing `request` field"))?;
            Ok(AppServerRequest::ServerRequest {
                request_id,
                conversation_id: serde_json::from_value(conversation_id)?,
                request: serde_json::from_value(request)?,
            })
        }
        other => anyhow::bail!("unsupported server request method: {other}"),
    }
}

fn value_field<T>(value: Value, field: &str) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("expected object params"))?;
    let value = object
        .get(field)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("missing `{field}` field"))?;
    Ok(serde_json::from_value(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_core_transcript_notifications_matches_codex_core_set() {
        let agent_delta = AppServerNotification::AgentMessageDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "assistant:1".to_string(),
            delta: "hello".to_string(),
        };
        let reasoning_summary = AppServerNotification::ReasoningSummaryTextDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            delta: "summary".to_string(),
        };
        let reasoning_text = AppServerNotification::ReasoningTextDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "reasoning:1".to_string(),
            delta: "detail".to_string(),
        };
        let plan_delta = AppServerNotification::PlanDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "plan:1".to_string(),
            delta: "step 1".to_string(),
        };
        let item_completed = AppServerNotification::ItemCompleted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "done".to_string(),
            },
        };
        let turn_completed = AppServerNotification::TurnCompleted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
        };

        for notification in [
            agent_delta,
            reasoning_summary,
            reasoning_text,
            plan_delta,
            item_completed,
            turn_completed,
        ] {
            assert_eq!(
                classify_notification(&notification),
                (
                    NotificationStream::CoreTranscript,
                    NotificationDelivery::Lossless
                )
            );
        }
    }

    #[test]
    fn command_execution_output_is_control_not_core_transcript() {
        for notification in [
            AppServerNotification::CommandExecutionOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "tool:1".to_string(),
                delta: "D:\\work".to_string(),
            },
            AppServerNotification::FileChangeOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "tool:2".to_string(),
                delta: "wrote D:\\work\\note.txt".to_string(),
            },
            AppServerNotification::ToolOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "tool:3".to_string(),
                delta: "generic tool output".to_string(),
            },
        ] {
            assert_eq!(
                classify_notification(&notification),
                (
                    NotificationStream::Control,
                    NotificationDelivery::BestEffort
                )
            );
        }
    }

    #[test]
    fn tool_output_roundtrips_through_jsonrpc_notification() {
        let message = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::ToolOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "tool:custom".to_string(),
                delta: "custom output".to_string(),
            }),
        };

        let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
            panic!("expected notification");
        };
        assert_eq!(notification.method, "item/tool/outputDelta");

        let reparsed =
            AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
                .expect("reparse");
        match reparsed.message {
            AppServerMessage::Notification(AppServerNotification::ToolOutputDelta {
                item_id,
                delta,
                ..
            }) => {
                assert_eq!(item_id, "tool:custom");
                assert_eq!(delta, "custom output");
            }
            other => panic!("unexpected notification: {other:?}"),
        }
    }

    #[test]
    fn file_change_output_roundtrips_through_jsonrpc_notification() {
        let message = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::FileChangeOutputDelta {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "tool:write".to_string(),
                delta: "wrote note.txt".to_string(),
            }),
        };

        let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
            panic!("expected notification");
        };
        assert_eq!(notification.method, "item/fileChange/outputDelta");

        let reparsed =
            AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
                .expect("reparse");
        match reparsed.message {
            AppServerMessage::Notification(AppServerNotification::FileChangeOutputDelta {
                item_id,
                delta,
                ..
            }) => {
                assert_eq!(item_id, "tool:write");
                assert_eq!(delta, "wrote note.txt");
            }
            other => panic!("unexpected notification: {other:?}"),
        }
    }

    #[test]
    fn approval_request_roundtrips_through_jsonrpc() {
        let message = AppServerMessageEnvelope {
            message: AppServerMessage::Request(AppServerRequest::ServerRequest {
                request_id: RequestId::Integer(7),
                conversation_id: "default".to_string(),
                request: ServerRequest::ToolApproval {
                    request: ToolApprovalRequest {
                        turn_id: "turn-1".to_string(),
                        tool_call_id: "call-1".to_string(),
                        tool_name: "shell_command".to_string(),
                        reason: "mutating tool".to_string(),
                        arguments_preview: "{\"command\":\"echo hi\"}".to_string(),
                    },
                },
            }),
        };

        let JsonRpcMessage::Request(request) = JsonRpcMessage::from(message) else {
            panic!("expected request");
        };
        assert_eq!(request.method, "serverRequest/toolApproval");
        assert_eq!(request.id, RequestId::Integer(7));

        let reparsed =
            AppServerMessageEnvelope::try_from(JsonRpcMessage::Request(request)).expect("reparse");
        match reparsed.message {
            AppServerMessage::Request(AppServerRequest::ServerRequest {
                request_id,
                request: ServerRequest::ToolApproval { request },
                ..
            }) => {
                assert_eq!(request_id, RequestId::Integer(7));
                assert_eq!(request.tool_name, "shell_command");
                assert_eq!(request.tool_call_id, "call-1");
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn submit_turn_roundtrips_from_jsonrpc_request() {
        let envelope = AppClientCommandEnvelope {
            request_id: RequestId::Integer(1),
            command: AppClientCommand::SubmitTurn(UserTurnInput {
                conversation_id: "default".to_string(),
                content: "hello".to_string(),
            }),
        };

        let rpc = JsonRpcMessage::from(envelope.clone());
        let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");

        assert_eq!(parsed.request_id, RequestId::Integer(1));
        match parsed.command {
            AppClientCommand::SubmitTurn(input) => {
                assert_eq!(input.conversation_id, "default");
                assert_eq!(input.content, "hello");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn resolve_server_request_roundtrips_from_jsonrpc_request() {
        let envelope = AppClientCommandEnvelope {
            request_id: RequestId::Integer(9),
            command: AppClientCommand::ResolveServerRequest {
                conversation_id: "default".to_string(),
                request_id: RequestId::Integer(7),
                approved: true,
                reason: Some("ok".to_string()),
            },
        };

        let rpc = JsonRpcMessage::from(envelope.clone());
        let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");

        assert_eq!(parsed.request_id, RequestId::Integer(9));
        match parsed.command {
            AppClientCommand::ResolveServerRequest {
                conversation_id,
                request_id,
                approved,
                reason,
            } => {
                assert_eq!(conversation_id, "default");
                assert_eq!(request_id, RequestId::Integer(7));
                assert!(approved);
                assert_eq!(reason.as_deref(), Some("ok"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn request_conversation_notifications_roundtrips_from_jsonrpc_request() {
        let envelope = AppClientCommandEnvelope {
            request_id: RequestId::Integer(11),
            command: AppClientCommand::RequestConversationNotifications {
                conversation_id: "default".to_string(),
                after_sequence: 42,
            },
        };

        let rpc = JsonRpcMessage::from(envelope.clone());
        let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");

        assert_eq!(parsed.request_id, RequestId::Integer(11));
        match parsed.command {
            AppClientCommand::RequestConversationNotifications {
                conversation_id,
                after_sequence,
            } => {
                assert_eq!(conversation_id, "default");
                assert_eq!(after_sequence, 42);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn conversation_notifications_roundtrips_through_jsonrpc_notification() {
        let message = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(
                AppServerNotification::ConversationNotifications {
                    conversation_id: "default".to_string(),
                    from_sequence: 2,
                    messages: vec![SequencedAppServerMessage {
                        sequence: 2,
                        message: AppServerMessage::Notification(
                            AppServerNotification::TurnCompleted {
                                conversation_id: "default".to_string(),
                                turn_id: "turn-1".to_string(),
                            },
                        ),
                    }],
                },
            ),
        };

        let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
            panic!("expected notification");
        };
        assert_eq!(notification.method, "conversation/notifications");

        let reparsed =
            AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
                .expect("reparse");
        match reparsed.message {
            AppServerMessage::Notification(AppServerNotification::ConversationNotifications {
                from_sequence,
                messages,
                ..
            }) => {
                assert_eq!(from_sequence, 2);
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].sequence, 2);
            }
            other => panic!("unexpected notification: {other:?}"),
        }
    }
}
