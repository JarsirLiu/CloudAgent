use crate::{
    GatewayApprovalRequest, GatewayMessage, GatewayOutbound, GatewayProgressKind,
    GatewayProgressUpdate,
};
use agent_core::TranscriptItem;
use agent_core::ServerRequestDecision;
use agent_protocol::{
    AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, TurnPolicy,
    UserTurnInput,
};

pub fn gateway_message_to_command(
    message: GatewayMessage,
    turn_policy: TurnPolicy,
) -> AppClientCommand {
    AppClientCommand::SubmitTurn(UserTurnInput {
        conversation_id: message.conversation_id,
        content: message.content,
        turn_policy,
    })
}

pub fn app_server_message_to_outbound(message: &AppServerMessage) -> Option<GatewayOutbound> {
    match message {
        AppServerMessage::Notification(notification) => notification_to_outbound(notification),
        AppServerMessage::Request(request) => request_to_outbound(request),
    }
}

fn notification_to_outbound(notification: &AppServerNotification) -> Option<GatewayOutbound> {
    match notification {
        AppServerNotification::AgentMessageDelta {
            conversation_id,
            delta,
            ..
        } => Some(GatewayOutbound::TextDelta {
            conversation_id: conversation_id.clone(),
            delta: delta.clone(),
        }),
        AppServerNotification::TurnCompleted {
            conversation_id, ..
        } => Some(GatewayOutbound::FlushText {
            conversation_id: conversation_id.clone(),
        }),
        AppServerNotification::ItemCompleted {
            conversation_id,
            item: TranscriptItem::AgentMessage { text, .. },
            ..
        } => Some(GatewayOutbound::FinalText {
            conversation_id: conversation_id.clone(),
            text: text.clone(),
        }),
        AppServerNotification::PlanDelta {
            conversation_id,
            delta,
            ..
        } => progress_outbound(
            conversation_id,
            GatewayProgressKind::Plan,
            delta,
            true,
        ),
        AppServerNotification::ReasoningSummaryTextDelta {
            conversation_id,
            delta,
            ..
        } => progress_outbound(
            conversation_id,
            GatewayProgressKind::Reasoning,
            delta,
            true,
        ),
        AppServerNotification::ReasoningTextDelta {
            conversation_id,
            delta,
            ..
        } => progress_outbound(
            conversation_id,
            GatewayProgressKind::Reasoning,
            delta,
            true,
        ),
        AppServerNotification::ItemCompleted {
            conversation_id,
            item,
            ..
        } => completed_item_to_outbound(conversation_id, item),
        AppServerNotification::ServerRequestResolved {
            conversation_id,
            decision,
            ..
        } => approval_resolution_notice(conversation_id, decision),
        AppServerNotification::CommandExecutionOutputDelta {
            conversation_id,
            delta,
            ..
        }
        | AppServerNotification::ToolOutputDelta {
            conversation_id,
            delta,
            ..
        }
        | AppServerNotification::FileChangeOutputDelta {
            conversation_id,
            delta,
            ..
        } => progress_outbound(
            conversation_id,
            GatewayProgressKind::Tool,
            delta,
            true,
        ),
        AppServerNotification::Info {
            conversation_id,
            message,
        } => Some(GatewayOutbound::Info {
            conversation_id: conversation_id.clone(),
            message: message.clone(),
        }),
        AppServerNotification::Error {
            conversation_id,
            message,
        } => Some(GatewayOutbound::Error {
            conversation_id: conversation_id.clone(),
            message: message.clone(),
        }),
        _ => None,
    }
}

fn completed_item_to_outbound(
    conversation_id: &str,
    item: &TranscriptItem,
) -> Option<GatewayOutbound> {
    match item {
        TranscriptItem::Reasoning { title, text, .. } => Some(GatewayOutbound::Progress(
            GatewayProgressUpdate {
                conversation_id: conversation_id.to_string(),
                kind: GatewayProgressKind::Reasoning,
                summary: format_reasoning_summary(title, text),
                streaming: false,
            },
        )),
        TranscriptItem::ToolResult {
            tool_name,
            summary,
            content,
            ..
        } => Some(GatewayOutbound::Progress(GatewayProgressUpdate {
            conversation_id: conversation_id.to_string(),
            kind: GatewayProgressKind::Tool,
            summary: format_completed_tool_message(tool_name, summary, content),
            streaming: false,
        })),
        TranscriptItem::CommandExecution {
            tool_name,
            summary,
            aggregated_output,
            ..
        } => Some(GatewayOutbound::Progress(GatewayProgressUpdate {
            conversation_id: conversation_id.to_string(),
            kind: GatewayProgressKind::Tool,
            summary: format_completed_tool_message(
                tool_name,
                summary,
                aggregated_output.as_deref().unwrap_or_default(),
            ),
            streaming: false,
        })),
        TranscriptItem::FileChange {
            tool_name,
            summary,
            path,
            ..
        } => Some(GatewayOutbound::Progress(GatewayProgressUpdate {
            conversation_id: conversation_id.to_string(),
            kind: GatewayProgressKind::Tool,
            summary: format_completed_tool_message(tool_name, summary, path),
            streaming: false,
        })),
        _ => None,
    }
}

fn progress_outbound(
    conversation_id: &str,
    kind: GatewayProgressKind,
    raw: &str,
    streaming: bool,
) -> Option<GatewayOutbound> {
    let summary = normalize_progress_text(raw);
    if summary.is_empty() {
        return None;
    }
    Some(GatewayOutbound::Progress(GatewayProgressUpdate {
        conversation_id: conversation_id.to_string(),
        kind,
        summary,
        streaming,
    }))
}

fn normalize_progress_text(raw: &str) -> String {
    let flattened = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    flattened.trim().to_string()
}

fn format_reasoning_summary(title: &str, text: &str) -> String {
    let title = title.trim();
    let text = normalize_progress_text(text);
    if title.is_empty() {
        return text;
    }
    if text.is_empty() {
        return title.to_string();
    }
    format!("{title}: {text}")
}

fn format_completed_tool_message(tool_name: &str, summary: &str, detail: &str) -> String {
    let tool_name = tool_name.trim();
    let summary = normalize_progress_text(summary);
    let detail = normalize_progress_text(detail);

    if !summary.is_empty() && !detail.is_empty() && summary != detail {
        return format!("{tool_name}: {summary}\n{detail}");
    }
    if !summary.is_empty() {
        return format!("{tool_name}: {summary}");
    }
    if !detail.is_empty() {
        return format!("{tool_name}: {detail}");
    }
    tool_name.to_string()
}

fn request_to_outbound(request: &AppServerRequest) -> Option<GatewayOutbound> {
    match request {
        AppServerRequest::ServerRequest {
            request_id,
            conversation_id,
            request,
        } => Some(GatewayOutbound::ApprovalRequest(GatewayApprovalRequest {
            conversation_id: conversation_id.clone(),
            request_id: request_id.clone(),
            request: request.clone(),
        })),
    }
}

fn approval_resolution_notice(
    conversation_id: &str,
    decision: &ServerRequestDecision,
) -> Option<GatewayOutbound> {
    Some(GatewayOutbound::Info {
        conversation_id: conversation_id.to_string(),
        message: format!("approval {}", decision.label()),
    })
}

#[cfg(test)]
mod tests {
    use super::{app_server_message_to_outbound, gateway_message_to_command};
    use crate::{GatewayMessage, GatewayOutbound};
    use agent_core::{
        ApprovalPolicy, CommandApprovalRequest, InputItem, PermissionProfile, ServerRequest,
        ServerRequestDecision,
    };
    use agent_protocol::{
        AppClientCommand, AppServerMessage, AppServerNotification, AppServerRequest, RequestId,
        TurnPolicy,
    };

    #[test]
    fn gateway_message_maps_to_submit_turn_command() {
        let command = gateway_message_to_command(
            GatewayMessage::new(
                "conversation-1",
                "sender-1",
                vec![InputItem::Text {
                    text: "hello".to_string(),
                }],
            ),
            TurnPolicy {
                permission_profile: PermissionProfile::ReadOnly,
                approval_policy: ApprovalPolicy::OnRequest,
            },
        );

        match command {
            AppClientCommand::SubmitTurn(input) => {
                assert_eq!(input.conversation_id, "conversation-1");
                assert_eq!(
                    input.content,
                    vec![InputItem::Text {
                        text: "hello".to_string()
                    }]
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn agent_delta_maps_to_text_outbound() {
        let outbound = app_server_message_to_outbound(&AppServerMessage::Notification(
            AppServerNotification::AgentMessageDelta {
                conversation_id: "conversation-1".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "assistant:1".to_string(),
                delta: "hello".to_string(),
            },
        ))
        .expect("text outbound");

        match outbound {
            GatewayOutbound::TextDelta {
                conversation_id,
                delta,
            } => {
                assert_eq!(conversation_id, "conversation-1");
                assert_eq!(delta, "hello");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[test]
    fn server_request_message_maps_to_gateway_approval_outbound() {
        let outbound = app_server_message_to_outbound(&AppServerMessage::Request(
            AppServerRequest::ServerRequest {
                request_id: RequestId::Integer(7),
                conversation_id: "conversation-1".to_string(),
                request: ServerRequest::CommandApproval {
                    request: CommandApprovalRequest {
                        turn_id: "turn-1".to_string(),
                        tool_call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        reason: "need approval".to_string(),
                        command_preview: "echo hi".to_string(),
                    },
                },
            },
        ))
        .expect("approval outbound");

        match outbound {
            GatewayOutbound::ApprovalRequest(request) => {
                assert_eq!(request.conversation_id, "conversation-1");
                assert_eq!(request.request_id, RequestId::Integer(7));
                assert!(matches!(
                    request.request,
                    ServerRequest::CommandApproval { .. }
                ));
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[test]
    fn completed_agent_message_maps_to_final_text_outbound() {
        let outbound = app_server_message_to_outbound(&AppServerMessage::Notification(
            AppServerNotification::ItemCompleted {
                conversation_id: "conversation-1".to_string(),
                turn_id: "turn-1".to_string(),
                call_id: None,
                item: agent_core::TranscriptItem::AgentMessage {
                    id: "assistant:1".to_string(),
                    text: "final answer".to_string(),
                },
            },
        ))
        .expect("final text outbound");

        match outbound {
            GatewayOutbound::FinalText {
                conversation_id,
                text,
            } => {
                assert_eq!(conversation_id, "conversation-1");
                assert_eq!(text, "final answer");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[test]
    fn approval_resolution_maps_to_info_outbound() {
        let outbound = app_server_message_to_outbound(&AppServerMessage::Notification(
            AppServerNotification::ServerRequestResolved {
                conversation_id: "conversation-1".to_string(),
                turn_id: "turn-1".to_string(),
                request_id: RequestId::Integer(1),
                request: ServerRequest::CommandApproval {
                    request: CommandApprovalRequest {
                        turn_id: "turn-1".to_string(),
                        tool_call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        reason: "need approval".to_string(),
                        command_preview: "echo hi".to_string(),
                    },
                },
                decision: ServerRequestDecision::accept(None),
            },
        ))
        .expect("resolution outbound");

        match outbound {
            GatewayOutbound::Info {
                conversation_id,
                message,
            } => {
                assert_eq!(conversation_id, "conversation-1");
                assert_eq!(message, "approval approved");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[test]
    fn turn_completed_maps_to_flush_text_outbound() {
        let outbound = app_server_message_to_outbound(&AppServerMessage::Notification(
            AppServerNotification::TurnCompleted {
                conversation_id: "conversation-1".to_string(),
                turn_id: "turn-1".to_string(),
            },
        ))
        .expect("flush outbound");

        match outbound {
            GatewayOutbound::FlushText { conversation_id } => {
                assert_eq!(conversation_id, "conversation-1");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }
}
