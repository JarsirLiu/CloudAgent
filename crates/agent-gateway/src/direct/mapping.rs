use crate::{GatewayApprovalRequest, GatewayMessage, GatewayOutbound};
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
        } => Some(GatewayOutbound::ToolNotice {
            conversation_id: conversation_id.clone(),
            message: delta.clone(),
        }),
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
    fn unrelated_messages_are_ignored() {
        let outbound = app_server_message_to_outbound(&AppServerMessage::Notification(
            AppServerNotification::TurnCompleted {
                conversation_id: "conversation-1".to_string(),
                turn_id: "turn-1".to_string(),
            },
        ));
        assert!(outbound.is_none());
    }
}
