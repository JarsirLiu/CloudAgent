mod jsonrpc;

pub use jsonrpc::{
    JsonRpcError, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type TurnId = String;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FrontendMode {
    Idle,
    Running,
    WaitingForApproval,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserTurnInput {
    pub session_id: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub turn_id: TurnId,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub arguments_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub approved: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TurnState {
    Idle,
    Running,
    WaitingForApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionState {
    Idle,
    Busy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub session_state: SessionState,
    pub active_turn: Option<TurnId>,
    pub turn_state: Option<TurnState>,
    pub message_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub mutating: bool,
    pub requires_approval: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
    pub summary: String,
    pub is_error: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistoryEntry {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: Option<String>,
        has_tool_calls: bool,
    },
    Tool {
        tool_call_id: String,
        name: String,
        content: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnResultEnvelope {
    pub final_response: String,
    pub state: TurnState,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemKind {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Reasoning,
    SystemNote,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TurnItemDeltaKind {
    Text,
    ToolOutput,
    ReasoningText,
    ReasoningSummary,
    JsonPatch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnEvent {
    TurnStarted {
        turn_id: TurnId,
        session_id: String,
        user_input: String,
    },
    ModelRequestStarted {
        turn_id: TurnId,
        message_count: usize,
        tool_count: usize,
    },
    ModelResponseReceived {
        turn_id: TurnId,
        model_name: Option<String>,
        has_content: bool,
        tool_call_count: usize,
    },
    ItemStarted {
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
    },
    ItemDelta {
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemDeltaKind,
        delta: String,
    },
    ItemCompleted {
        turn_id: TurnId,
        item_id: String,
    },
    ApprovalRequested {
        turn_id: TurnId,
        request: ApprovalRequest,
    },
    ApprovalResolved {
        turn_id: TurnId,
        tool_call_id: String,
        approved: bool,
        reason: Option<String>,
    },
    TurnCompleted {
        turn_id: TurnId,
        final_response: String,
    },
    TurnFailed {
        turn_id: TurnId,
        error: String,
    },
    TurnCancelled {
        turn_id: TurnId,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppClientCommand {
    SubmitTurn(UserTurnInput),
    ApprovalResponse {
        session_id: String,
        request_id: RequestId,
        approved: bool,
        reason: Option<String>,
    },
    InterruptTurn {
        session_id: String,
    },
    ResetSession {
        session_id: String,
    },
    RequestStatus {
        session_id: String,
    },
    RequestHistory {
        session_id: String,
    },
    SubscribeSession {
        session_id: String,
    },
    UnsubscribeSession {
        session_id: String,
    },
    Exit,
}

impl AppClientCommand {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::SubmitTurn(input) => Some(&input.session_id),
            Self::ApprovalResponse { session_id, .. }
            | Self::InterruptTurn { session_id }
            | Self::ResetSession { session_id }
            | Self::RequestStatus { session_id }
            | Self::RequestHistory { session_id }
            | Self::SubscribeSession { session_id }
            | Self::UnsubscribeSession { session_id } => Some(session_id),
            Self::Exit => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerNotification {
    FrontendStateChanged {
        session_id: String,
        mode: FrontendMode,
    },
    TurnStarted {
        session_id: String,
        turn_id: TurnId,
    },
    ItemStarted {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        kind: TurnItemKind,
        title: Option<String>,
    },
    AgentMessageDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    PlanDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningSummaryDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ReasoningTextDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ToolCallDelta {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
        delta: String,
    },
    ItemCompleted {
        session_id: String,
        turn_id: TurnId,
        item_id: String,
    },
    TurnCompleted {
        session_id: String,
        turn_id: TurnId,
        final_response: String,
    },
    TurnFailed {
        session_id: String,
        turn_id: TurnId,
        error: String,
    },
    TurnCancelled {
        session_id: String,
        turn_id: TurnId,
        reason: String,
    },
    SessionStatus {
        session_id: String,
        snapshot: SessionSnapshot,
    },
    SessionHistory {
        session_id: String,
        messages: Vec<HistoryEntry>,
    },
    TurnFinished {
        session_id: String,
        result: TurnResultEnvelope,
    },
    SubscriptionChanged {
        session_id: String,
        subscribed: bool,
    },
    Info {
        session_id: String,
        message: String,
    },
    Error {
        session_id: String,
        message: String,
    },
}

impl AppServerNotification {
    pub fn session_id(&self) -> &str {
        match self {
            Self::FrontendStateChanged { session_id, .. }
            | Self::TurnStarted { session_id, .. }
            | Self::ItemStarted { session_id, .. }
            | Self::AgentMessageDelta { session_id, .. }
            | Self::PlanDelta { session_id, .. }
            | Self::ReasoningSummaryDelta { session_id, .. }
            | Self::ReasoningTextDelta { session_id, .. }
            | Self::ToolCallDelta { session_id, .. }
            | Self::ItemCompleted { session_id, .. }
            | Self::TurnCompleted { session_id, .. }
            | Self::TurnFailed { session_id, .. }
            | Self::TurnCancelled { session_id, .. }
            | Self::SessionStatus { session_id, .. }
            | Self::SessionHistory { session_id, .. }
            | Self::TurnFinished { session_id, .. }
            | Self::SubscriptionChanged { session_id, .. }
            | Self::Info { session_id, .. }
            | Self::Error { session_id, .. } => session_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerRequest {
    Approval {
        request_id: RequestId,
        session_id: String,
        request: ApprovalRequest,
    },
}

impl AppServerRequest {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::Approval { request_id, .. } => request_id,
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::Approval { session_id, .. } => session_id,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppServerMessage {
    Notification(AppServerNotification),
    Request(AppServerRequest),
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
            session_id: value_field(params, "session_id")?,
        }),
        "session/reset" => Ok(AppClientCommand::ResetSession {
            session_id: value_field(params, "session_id")?,
        }),
        "session/status" => Ok(AppClientCommand::RequestStatus {
            session_id: value_field(params, "session_id")?,
        }),
        "session/history" => Ok(AppClientCommand::RequestHistory {
            session_id: value_field(params, "session_id")?,
        }),
        "session/subscribe" => Ok(AppClientCommand::SubscribeSession {
            session_id: value_field(params, "session_id")?,
        }),
        "session/unsubscribe" => Ok(AppClientCommand::UnsubscribeSession {
            session_id: value_field(params, "session_id")?,
        }),
        "approval/respond" => Ok(serde_json::from_value(params)?),
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
        AppClientCommand::ApprovalResponse { .. } => (
            "approval/respond",
            serde_json::to_value(command).unwrap_or(Value::Null),
        ),
        AppClientCommand::InterruptTurn { session_id } => (
            "turn/interrupt",
            serde_json::json!({ "session_id": session_id }),
        ),
        AppClientCommand::ResetSession { session_id } => (
            "session/reset",
            serde_json::json!({ "session_id": session_id }),
        ),
        AppClientCommand::RequestStatus { session_id } => (
            "session/status",
            serde_json::json!({ "session_id": session_id }),
        ),
        AppClientCommand::RequestHistory { session_id } => (
            "session/history",
            serde_json::json!({ "session_id": session_id }),
        ),
        AppClientCommand::SubscribeSession { session_id } => (
            "session/subscribe",
            serde_json::json!({ "session_id": session_id }),
        ),
        AppClientCommand::UnsubscribeSession { session_id } => (
            "session/unsubscribe",
            serde_json::json!({ "session_id": session_id }),
        ),
        AppClientCommand::Exit => ("app/exit", Value::Null),
    }
}

fn notification_method_and_params(notification: &AppServerNotification) -> (&'static str, Value) {
    match notification {
        AppServerNotification::FrontendStateChanged { .. } => (
            "frontend/state_changed",
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
            "item/agent_message/delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::PlanDelta { .. } => (
            "item/plan/delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ReasoningSummaryDelta { .. } => (
            "item/reasoning/summary_text_delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ReasoningTextDelta { .. } => (
            "item/reasoning/text_delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ToolCallDelta { .. } => (
            "item/tool_call/delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ItemCompleted { .. } => (
            "item/completed",
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
        AppServerNotification::SessionStatus { .. } => (
            "session/status",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::SessionHistory { .. } => (
            "session/history",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::TurnFinished { .. } => (
            "turn/finished",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::SubscriptionChanged { .. } => (
            "session/subscription_changed",
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
        AppServerRequest::Approval {
            request_id,
            session_id,
            request,
        } => (
            request_id.clone(),
            "approval/request",
            serde_json::json!({
                "session_id": session_id,
                "request": request,
            }),
        ),
    }
}

fn parse_server_notification(
    method: &str,
    params: Option<Value>,
) -> anyhow::Result<AppServerNotification> {
    let params = params.unwrap_or(Value::Null);
    match method {
        "frontend/state_changed"
        | "turn/started"
        | "item/started"
        | "item/agent_message/delta"
        | "item/plan/delta"
        | "item/reasoning/summary_text_delta"
        | "item/reasoning/text_delta"
        | "item/tool_call/delta"
        | "item/completed"
        | "turn/completed"
        | "turn/failed"
        | "turn/cancelled"
        | "session/status"
        | "session/history"
        | "turn/finished"
        | "session/subscription_changed"
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
        "approval/request" => {
            let object = params
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("expected object params"))?;
            let session_id = object
                .get("session_id")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing `session_id` field"))?;
            let request = object
                .get("request")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing `request` field"))?;
            Ok(AppServerRequest::Approval {
                request_id,
                session_id: serde_json::from_value(session_id)?,
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
    fn approval_request_roundtrips_through_jsonrpc() {
        let message = AppServerMessageEnvelope {
            message: AppServerMessage::Request(AppServerRequest::Approval {
                request_id: RequestId::Integer(7),
                session_id: "default".to_string(),
                request: ApprovalRequest {
                    turn_id: "turn-1".to_string(),
                    tool_call_id: "call-1".to_string(),
                    tool_name: "shell_command".to_string(),
                    reason: "mutating tool".to_string(),
                    arguments_preview: "{\"command\":\"echo hi\"}".to_string(),
                },
            }),
        };

        let JsonRpcMessage::Request(request) = JsonRpcMessage::from(message) else {
            panic!("expected request");
        };
        assert_eq!(request.method, "approval/request");
        assert_eq!(request.id, RequestId::Integer(7));
    }

    #[test]
    fn submit_turn_roundtrips_from_jsonrpc_request() {
        let envelope = AppClientCommandEnvelope {
            request_id: RequestId::Integer(1),
            command: AppClientCommand::SubmitTurn(UserTurnInput {
                session_id: "default".to_string(),
                content: "hello".to_string(),
            }),
        };

        let rpc = JsonRpcMessage::from(envelope.clone());
        let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");

        assert_eq!(parsed.request_id, RequestId::Integer(1));
        match parsed.command {
            AppClientCommand::SubmitTurn(input) => {
                assert_eq!(input.session_id, "default");
                assert_eq!(input.content, "hello");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
