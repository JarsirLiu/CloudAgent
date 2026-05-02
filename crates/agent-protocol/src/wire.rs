use crate::*;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

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
        "conversation/compact" => Ok(AppClientCommand::CompactConversation {
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
        AppClientCommand::RequestConversationStatus { conversation_id } => (
            "conversation/status",
            serde_json::json!({ "conversation_id": conversation_id }),
        ),
        AppClientCommand::RequestConversationHistory { conversation_id } => (
            "conversation/history",
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
        AppServerNotification::TokenUsageUpdated { .. } => (
            "turn/tokenUsageUpdated",
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
        AppServerNotification::ConversationStatus { .. } => (
            "conversation/status",
            serde_json::to_value(notification).unwrap_or(Value::Null),
        ),
        AppServerNotification::ConversationHistory { .. } => (
            "conversation/history",
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
        | "turn/tokenUsageUpdated"
        | "turn/contextCompacted"
        | "turn/contextCompactionStarted"
        | "item/jsonPatch/delta"
        | "item/completed"
        | "serverRequest/requested"
        | "serverRequest/resolved"
        | "turn/completed"
        | "turn/failed"
        | "turn/cancelled"
        | "conversation/status"
        | "conversation/history"
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
    fn token_usage_roundtrips_through_jsonrpc_notification() {
        let message = AppServerMessageEnvelope {
            message: AppServerMessage::Notification(AppServerNotification::TokenUsageUpdated {
                conversation_id: "default".to_string(),
                turn_id: "turn-1".to_string(),
                last_usage: ModelUsage {
                    input_tokens: 10,
                    cached_input_tokens: 3,
                    output_tokens: 5,
                    reasoning_output_tokens: 1,
                    total_tokens: 15,
                },
                total_usage: ModelUsage {
                    input_tokens: 20,
                    cached_input_tokens: 6,
                    output_tokens: 10,
                    reasoning_output_tokens: 2,
                    total_tokens: 30,
                },
                model_context_window: Some(100),
            }),
        };

        let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
            panic!("expected notification");
        };
        assert_eq!(notification.method, "turn/tokenUsageUpdated");

        let reparsed =
            AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
                .expect("reparse");
        match reparsed.message {
            AppServerMessage::Notification(AppServerNotification::TokenUsageUpdated {
                last_usage,
                total_usage,
                model_context_window,
                ..
            }) => {
                assert_eq!(last_usage.total_tokens, 15);
                assert_eq!(total_usage.cached_input_tokens, 6);
                assert_eq!(model_context_window, Some(100));
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
                decision: ServerRequestDecision::accept(Some("ok".to_string())),
            },
        };

        let rpc = JsonRpcMessage::from(envelope.clone());
        let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");

        assert_eq!(parsed.request_id, RequestId::Integer(9));
        match parsed.command {
            AppClientCommand::ResolveServerRequest {
                conversation_id,
                request_id,
                decision,
            } => {
                assert_eq!(conversation_id, "default");
                assert_eq!(request_id, RequestId::Integer(7));
                assert!(decision.is_approved());
                assert_eq!(decision.reason.as_deref(), Some("ok"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
