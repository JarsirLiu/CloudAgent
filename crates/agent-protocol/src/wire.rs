use crate::*;
use agent_core::ServerRequest;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppClientCommandEnvelope {
    pub request_id: RequestId,
    pub command: AppClientCommand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<CommandExecutionContext>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppServerMessageEnvelope {
    pub message: AppServerMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_seq: Option<u64>,
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
        JsonRpcMessage::Request(envelope.into_request())
    }
}

impl AppClientCommandEnvelope {
    pub fn into_request(self) -> JsonRpcRequest {
        let AppClientCommandEnvelope {
            request_id,
            command,
            context,
        } = self;
        let (method, mut params) = command_method_and_params(&command);
        if let Some(context) = context {
            if !params.is_object() {
                params = serde_json::json!({});
            }
            if let Some(object) = params.as_object_mut() {
                object.insert(
                    "_context".to_string(),
                    serde_json::to_value(context).unwrap_or(Value::Null),
                );
            }
        }
        JsonRpcRequest {
            id: request_id,
            method: method.to_string(),
            params: Some(params),
        }
    }

    pub fn into_notification(self) -> JsonRpcNotification {
        let request = self.into_request();
        JsonRpcNotification {
            method: request.method,
            params: request.params,
        }
    }
}

impl From<AppServerMessageEnvelope> for JsonRpcMessage {
    fn from(envelope: AppServerMessageEnvelope) -> Self {
        match envelope.message {
            AppServerMessage::Notification(notification) => {
                let (method, params) = notification_method_and_params(&notification);
                let params = inject_event_seq(params, envelope.event_seq);
                JsonRpcMessage::Notification(JsonRpcNotification {
                    method: method.to_string(),
                    params: Some(params),
                })
            }
            AppServerMessage::Request(request) => {
                let (id, method, params) = request_method_and_params(&request);
                let params = inject_event_seq(params, envelope.event_seq);
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
            JsonRpcMessage::Notification(notification) => {
                let (event_seq, params) = extract_event_seq(notification.params);
                Ok(AppServerMessageEnvelope {
                    message: AppServerMessage::Notification(parse_server_notification(
                        &notification.method,
                        params,
                    )?),
                    event_seq,
                })
            }
            JsonRpcMessage::Request(request) => {
                let (event_seq, params) = extract_event_seq(request.params);
                Ok(AppServerMessageEnvelope {
                    message: AppServerMessage::Request(parse_server_request(
                        request.id,
                        &request.method,
                        params,
                    )?),
                    event_seq,
                })
            }
            JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {
                anyhow::bail!("server envelope expects a notification or request")
            }
        }
    }
}

fn inject_event_seq(mut params: Value, event_seq: Option<u64>) -> Value {
    if let Some(seq) = event_seq {
        if !params.is_object() {
            params = serde_json::json!({});
        }
        if let Some(object) = params.as_object_mut() {
            object.insert("_event_seq".to_string(), Value::from(seq));
        }
    }
    params
}

fn extract_event_seq(params: Option<Value>) -> (Option<u64>, Option<Value>) {
    let mut params = params;
    let mut event_seq = None;
    if let Some(Value::Object(object)) = params.as_mut()
        && let Some(seq) = object.remove("_event_seq")
    {
        event_seq = seq.as_u64();
    }
    (event_seq, params)
}

fn command_from_request(request: JsonRpcRequest) -> anyhow::Result<AppClientCommandEnvelope> {
    let (context, params) = extract_command_context(request.params);
    let command = parse_command(&request.method, params)?;
    Ok(AppClientCommandEnvelope {
        request_id: request.id,
        command,
        context,
    })
}

fn command_from_notification(
    notification: JsonRpcNotification,
) -> anyhow::Result<AppClientCommandEnvelope> {
    let (context, params) = extract_command_context(notification.params);
    let command = parse_command(&notification.method, params)?;
    Ok(AppClientCommandEnvelope {
        request_id: RequestId::String("notification".to_string()),
        command,
        context,
    })
}

fn extract_command_context(
    params: Option<Value>,
) -> (Option<CommandExecutionContext>, Option<Value>) {
    let mut params = params;
    let mut context = None;
    if let Some(Value::Object(object)) = params.as_mut()
        && let Some(raw_context) = object.remove("_context")
    {
        context = serde_json::from_value(raw_context).ok();
    }
    (context, params)
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
        "conversation/compact" => Ok(AppClientCommand::CompactConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/reset" => Ok(AppClientCommand::ResetConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/view" => Ok(AppClientCommand::RequestConversationView {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/history" => Ok(AppClientCommand::RequestConversationHistory {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/historyPage" => Ok(AppClientCommand::RequestConversationHistoryPage {
            conversation_id: value_field(params.clone(), "conversation_id")?,
            before_turn_id: optional_value_field(params.clone(), "before_turn_id")?,
            limit: optional_value_field(params, "limit")?.unwrap_or(30),
        }),
        "conversation/listPage" => Ok(AppClientCommand::ListConversationsPage {
            cursor: optional_value_field(params.clone(), "cursor")?,
            limit: optional_value_field(params, "limit")?.unwrap_or(25),
        }),
        "skills/list" => Ok(AppClientCommand::ListSkills),
        "hub/node/list" => Ok(AppClientCommand::ListOnlineNodes),
        "platform/list" => Ok(AppClientCommand::ListPlatforms),
        "node/status" => Ok(AppClientCommand::GetNodeStatus),
        "node/stop" => Ok(AppClientCommand::StopNode),
        "conversation/create" => Ok(AppClientCommand::CreateConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/title/set" => Ok(AppClientCommand::SetConversationTitle {
            conversation_id: value_field(params.clone(), "conversation_id")?,
            title: value_field(params, "title")?,
        }),
        "conversation/switch" => Ok(AppClientCommand::SwitchConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "hub/node/select" => Ok(AppClientCommand::SelectTargetNode {
            node_id: value_field(params, "node_id")?,
        }),
        "platform/status" => Ok(AppClientCommand::GetPlatformStatus {
            platform: value_field(params, "platform")?,
        }),
        "platform/config" => Ok(AppClientCommand::GetPlatformConfig {
            platform: value_field(params, "platform")?,
        }),
        "platform/setEnabled" => Ok(AppClientCommand::SetPlatformEnabled {
            platform: value_field(params.clone(), "platform")?,
            enabled: value_field(params, "enabled")?,
        }),
        "platform/config/set" => Ok(AppClientCommand::SetPlatformConfigValue {
            platform: value_field(params.clone(), "platform")?,
            key: value_field(params.clone(), "key")?,
            value: value_field(params, "value")?,
        }),
        "platform/config/clear" => Ok(AppClientCommand::ClearPlatformConfigValue {
            platform: value_field(params.clone(), "platform")?,
            key: value_field(params, "key")?,
        }),
        "llm/config/reload" => Ok(AppClientCommand::ReloadLlmConfig {
            api_key: value_field(params.clone(), "api_key")?,
            base_url: value_field(params.clone(), "base_url")?,
            model: value_field(params, "model")?,
        }),
        "weixin/login/start" => Ok(AppClientCommand::StartWeixinLogin),
        "weixin/login/check" => Ok(AppClientCommand::CheckWeixinLogin {
            session_id: value_field(params, "session_id")?,
        }),
        "conversation/archive" => Ok(AppClientCommand::ArchiveConversation {
            conversation_id: value_field(params, "conversation_id")?,
        }),
        "conversation/delete" => Ok(AppClientCommand::DeleteConversation {
            conversation_id: value_field(params, "conversation_id")?,
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
            decision: value_field(params, "decision")?,
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
        AppClientCommand::CompactConversation { conversation_id } => (
            "conversation/compact",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::ResetConversation { conversation_id } => (
            "conversation/reset",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationView { conversation_id } => (
            "conversation/view",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationHistory { conversation_id } => (
            "conversation/history",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationHistoryPage {
            conversation_id,
            before_turn_id,
            limit,
        } => (
            "conversation/historyPage",
            serde_json::json!({
                "conversation_id": conversation_id,
                "before_turn_id": before_turn_id,
                "limit": limit
            }),
        ),
        AppClientCommand::ListConversationsPage { cursor, limit } => (
            "conversation/listPage",
            serde_json::json!({
                "cursor": cursor,
                "limit": limit
            }),
        ),
        AppClientCommand::ListSkills => ("skills/list", Value::Null),
        AppClientCommand::ListOnlineNodes => ("hub/node/list", Value::Null),
        AppClientCommand::ListPlatforms => ("platform/list", Value::Null),
        AppClientCommand::GetNodeStatus => ("node/status", Value::Null),
        AppClientCommand::StopNode => ("node/stop", Value::Null),
        AppClientCommand::CreateConversation { conversation_id } => (
            "conversation/create",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::SetConversationTitle {
            conversation_id,
            title,
        } => (
            "conversation/title/set",
            serde_json::json!({ "conversation_id": conversation_id, "title": title }),
        ),
        AppClientCommand::SwitchConversation { conversation_id } => (
            "conversation/switch",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::SelectTargetNode { node_id } => {
            ("hub/node/select", serde_json::json!({ "node_id": node_id }))
        }
        AppClientCommand::GetPlatformStatus { platform } => (
            "platform/status",
            serde_json::json!({ "platform": platform }),
        ),
        AppClientCommand::GetPlatformConfig { platform } => (
            "platform/config",
            serde_json::json!({ "platform": platform }),
        ),
        AppClientCommand::SetPlatformEnabled { platform, enabled } => (
            "platform/setEnabled",
            serde_json::json!({ "platform": platform, "enabled": enabled }),
        ),
        AppClientCommand::SetPlatformConfigValue {
            platform,
            key,
            value,
        } => (
            "platform/config/set",
            serde_json::json!({ "platform": platform, "key": key, "value": value }),
        ),
        AppClientCommand::ClearPlatformConfigValue { platform, key } => (
            "platform/config/clear",
            serde_json::json!({ "platform": platform, "key": key }),
        ),
        AppClientCommand::ReloadLlmConfig {
            api_key,
            base_url,
            model,
        } => (
            "llm/config/reload",
            serde_json::json!({
                "api_key": api_key,
                "base_url": base_url,
                "model": model,
            }),
        ),
        AppClientCommand::StartWeixinLogin => ("weixin/login/start", Value::Null),
        AppClientCommand::CheckWeixinLogin { session_id } => (
            "weixin/login/check",
            serde_json::json!({ "session_id": session_id }),
        ),
        AppClientCommand::ArchiveConversation { conversation_id } => (
            "conversation/archive",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::DeleteConversation { conversation_id } => (
            "conversation/delete",
            serde_json::json!({ "conversation_id": conversation_id }),
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
        AppServerNotification::ConversationViewChanged { .. } => (
            "conversation/viewChanged",
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
        AppServerNotification::ReasoningSummaryPartAdded { .. } => (
            "item/reasoning/summaryPartAdded",
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
        AppServerNotification::JsonPatchDelta { .. } => (
            "item/jsonPatch/delta",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ItemProgress { .. } => (
            "item/progress",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ItemMetricsUpdated { .. } => (
            "item/metricsUpdated",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::TokenUsageUpdated { .. } => (
            "turn/tokenUsageUpdated",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ModelRetrying { .. } => (
            "turn/modelRetrying",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ContextCompacted { .. } => (
            "turn/contextCompacted",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ContextCompactionStarted { .. } => (
            "turn/contextCompactionStarted",
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
        AppServerNotification::InterruptResult { .. } => (
            "turn/interruptResult",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationHistory { .. } => (
            "conversation/history",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::TurnSnapshot { .. } => (
            "conversation/turnSnapshot",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationHistoryPage { .. } => (
            "conversation/historyPage",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationListPage { .. } => (
            "conversation/listPage",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::SkillsChanged { .. } => (
            "skills/changed",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::OnlineNodeList { .. } => (
            "hub/node/list",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationSwitched { .. } => (
            "conversation/switched",
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
            ServerRequest::CommandApproval { .. } => (
                request_id.clone(),
                "serverRequest/commandApproval",
                serde_json::json!({
                    "conversation_id": conversation_id,
                    "request": request,
                }),
            ),
            ServerRequest::FileChangeApproval { .. } => (
                request_id.clone(),
                "serverRequest/fileChangeApproval",
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
        "conversation/viewChanged"
        | "turn/started"
        | "item/started"
        | "item/agentMessage/delta"
        | "item/plan/delta"
        | "item/reasoning/summaryTextDelta"
        | "item/reasoning/textDelta"
        | "item/commandExecution/outputDelta"
        | "item/tool/outputDelta"
        | "item/progress"
        | "item/metricsUpdated"
        | "turn/tokenUsageUpdated"
        | "turn/modelRetrying"
        | "turn/contextCompacted"
        | "turn/contextCompactionStarted"
        | "item/jsonPatch/delta"
        | "item/completed"
        | "serverRequest/requested"
        | "serverRequest/resolved"
        | "turn/completed"
        | "turn/failed"
        | "turn/cancelled"
        | "turn/interruptResult"
        | "conversation/history"
        | "conversation/turnSnapshot"
        | "conversation/historyPage"
        | "conversation/listPage"
        | "skills/changed"
        | "hub/node/list"
        | "conversation/switched"
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
        "serverRequest/commandApproval" | "serverRequest/fileChangeApproval" => {
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

fn optional_value_field<T>(value: Value, field: &str) -> anyhow::Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("expected object params"))?;
    let Some(value) = object.get(field).cloned() else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(value)?))
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
