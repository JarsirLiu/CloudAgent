use super::*;
use agent_core::{
    ApprovalPolicy, CommandApprovalRequest, ConversationSummary, InputItem, ModelUsage,
    PermissionProfile, RuntimeItem, ServerRequestDecision, TranscriptItem, TurnItemKind,
};

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
        summary_index: 0,
        delta: "summary".to_string(),
    };
    let reasoning_text = AppServerNotification::ReasoningTextDelta {
        conversation_id: "default".to_string(),
        turn_id: "turn-1".to_string(),
        item_id: "reasoning:1".to_string(),
        content_index: 0,
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
        item: RuntimeItem::completed(
            &TranscriptItem::AgentMessage {
                id: "assistant:1".to_string(),
                text: "done".to_string(),
            },
            None,
        ),
        transcript_item: TranscriptItem::AgentMessage {
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
            call_id: None,
            delta: "D:\\work".to_string(),
        },
        AppServerNotification::ToolOutputDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool:2".to_string(),
            call_id: None,
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
            call_id: Some("call-1".to_string()),
            delta: "custom output".to_string(),
        }),
        event_seq: None,
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "item/tool/outputDelta");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::ToolOutputDelta {
            item_id,
            call_id,
            delta,
            ..
        }) => {
            assert_eq!(item_id, "tool:custom");
            assert_eq!(call_id.as_deref(), Some("call-1"));
            assert_eq!(delta, "custom output");
        }
        other => panic!("unexpected notification: {other:?}"),
    }
}

#[test]
fn conversation_view_changed_roundtrips_through_jsonrpc_notification() {
    let message = AppServerMessageEnvelope {
        message: AppServerMessage::Notification(AppServerNotification::ConversationViewChanged {
            conversation_id: "default".to_string(),
            snapshot: ConversationViewSnapshot {
                conversation_id: "default".to_string(),
                status: ConversationViewStatus::Active {
                    active_turn_id: Some("turn-1".to_string()),
                    flags: vec![ConversationActiveFlag::RunningTurn],
                },
                active_turn: None,
                pending_requests: Vec::new(),
                message_count: 3,
                updated_at_ms: 42,
            },
        }),
        event_seq: Some(12),
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "conversation/viewChanged");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    assert_eq!(reparsed.event_seq, Some(12));
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::ConversationViewChanged {
            conversation_id,
            snapshot,
        }) => {
            assert_eq!(conversation_id, "default");
            assert_eq!(snapshot.conversation_id, "default");
            assert_eq!(snapshot.message_count, 3);
            assert!(matches!(
                snapshot.status,
                ConversationViewStatus::Active { .. }
            ));
        }
        other => panic!("unexpected notification: {other:?}"),
    }
}

#[test]
fn conversation_list_page_roundtrips_through_jsonrpc_notification() {
    let message = AppServerMessageEnvelope {
        message: AppServerMessage::Notification(AppServerNotification::ConversationListPage {
            conversation_id: "default".to_string(),
            conversations: vec![ConversationSummary {
                conversation_id: "session-1".to_string(),
                title: Some("hello".to_string()),
                message_count: 3,
                updated_at_ms: 42,
            }],
            has_more: true,
            next_cursor: Some("42:session-1".to_string()),
        }),
        event_seq: Some(9),
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "conversation/listPage");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    assert_eq!(reparsed.event_seq, Some(9));
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::ConversationListPage {
            conversation_id,
            conversations,
            has_more,
            next_cursor,
        }) => {
            assert_eq!(conversation_id, "default");
            assert_eq!(conversations.len(), 1);
            assert_eq!(conversations[0].conversation_id, "session-1");
            assert!(has_more);
            assert_eq!(next_cursor.as_deref(), Some("42:session-1"));
        }
        other => panic!("unexpected notification: {other:?}"),
    }
}

#[test]
fn json_patch_delta_roundtrips_through_jsonrpc_notification() {
    let message = AppServerMessageEnvelope {
        message: AppServerMessage::Notification(AppServerNotification::JsonPatchDelta {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool:write".to_string(),
            call_id: Some("call-write".to_string()),
            delta: "*** Begin Patch\n*** End Patch".to_string(),
        }),
        event_seq: None,
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "item/jsonPatch/delta");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::JsonPatchDelta {
            item_id,
            call_id,
            delta,
            ..
        }) => {
            assert_eq!(item_id, "tool:write");
            assert_eq!(call_id.as_deref(), Some("call-write"));
            assert_eq!(delta, "*** Begin Patch\n*** End Patch");
        }
        other => panic!("unexpected notification: {other:?}"),
    }
}

#[test]
fn item_started_roundtrips_with_call_id() {
    let message = AppServerMessageEnvelope {
        message: AppServerMessage::Notification(AppServerNotification::ItemStarted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: RuntimeItem::started(
                "tool:custom",
                Some("call-1".to_string()),
                TurnItemKind::ToolResult,
                Some("read_file".to_string()),
            ),
        }),
        event_seq: None,
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "item/started");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::ItemStarted { item, .. }) => {
            assert_eq!(item.id, "tool:custom");
            assert_eq!(item.call_id.as_deref(), Some("call-1"));
            assert!(matches!(item.kind, TurnItemKind::ToolResult));
            assert_eq!(item.title.as_deref(), Some("read_file"));
        }
        other => panic!("unexpected notification: {other:?}"),
    }
}

#[test]
fn item_completed_roundtrips_with_call_id() {
    let transcript_item = TranscriptItem::ToolResult {
        id: "tool:custom".to_string(),
        tool_name: "read_file".to_string(),
        content: "ok".to_string(),
        summary: "ok".to_string(),
        structured: None,
    };
    let message = AppServerMessageEnvelope {
        message: AppServerMessage::Notification(AppServerNotification::ItemCompleted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: RuntimeItem::completed(&transcript_item, Some("call-1".to_string())),
            transcript_item,
        }),
        event_seq: None,
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "item/completed");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::ItemCompleted {
            item,
            transcript_item,
            ..
        }) => {
            assert_eq!(item.call_id.as_deref(), Some("call-1"));
            assert_eq!(item.title.as_deref(), Some("read_file"));
            assert!(matches!(
                transcript_item,
                TranscriptItem::ToolResult { ref tool_name, .. } if tool_name == "read_file"
            ));
        }
        other => panic!("unexpected notification: {other:?}"),
    }
}

#[test]
fn notifications_allow_missing_call_id() {
    let message = AppServerMessageEnvelope {
        message: AppServerMessage::Notification(AppServerNotification::ItemStarted {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item: RuntimeItem::started(
                "assistant:1",
                None,
                TurnItemKind::AssistantMessage,
                Some("assistant_message".to_string()),
            ),
        }),
        event_seq: None,
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::ItemStarted { item, .. }) => {
            assert!(item.call_id.is_none());
            assert_eq!(item.id, "assistant:1");
            assert!(matches!(item.kind, TurnItemKind::AssistantMessage));
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
        event_seq: None,
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "turn/tokenUsageUpdated");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
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
            request: ServerRequest::CommandApproval {
                request: CommandApprovalRequest {
                    turn_id: "turn-1".to_string(),
                    tool_call_id: "call-1".to_string(),
                    tool_name: "exec_command".to_string(),
                    reason: "mutating tool".to_string(),
                    command_preview: "{\"command\":\"echo hi\"}".to_string(),
                },
            },
        }),
        event_seq: None,
    };

    let JsonRpcMessage::Request(request) = JsonRpcMessage::from(message) else {
        panic!("expected request");
    };
    assert_eq!(request.method, "serverRequest/commandApproval");
    assert_eq!(request.id, RequestId::Integer(7));

    let reparsed =
        AppServerMessageEnvelope::try_from(JsonRpcMessage::Request(request)).expect("reparse");
    match reparsed.message {
        AppServerMessage::Request(AppServerRequest::ServerRequest {
            request_id,
            request: ServerRequest::CommandApproval { request },
            ..
        }) => {
            assert_eq!(request_id, RequestId::Integer(7));
            assert_eq!(request.tool_name, "exec_command");
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
            content: vec![InputItem::Text {
                text: "hello".to_string(),
            }],
            turn_policy: TurnPolicy {
                permission_profile: PermissionProfile::ReadOnly,
                approval_policy: ApprovalPolicy::OnRequest,
            },
        }),
        context: None,
    };

    let rpc = JsonRpcMessage::from(envelope.clone());
    let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");

    assert_eq!(parsed.request_id, RequestId::Integer(1));
    match parsed.command {
        AppClientCommand::SubmitTurn(input) => {
            assert_eq!(input.conversation_id, "default");
            assert_eq!(
                input.content,
                vec![InputItem::Text {
                    text: "hello".to_string()
                }]
            );
            assert!(matches!(
                input.turn_policy.permission_profile,
                PermissionProfile::ReadOnly
            ));
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
        context: None,
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

#[test]
fn history_page_roundtrips_from_jsonrpc_request() {
    let envelope = AppClientCommandEnvelope {
        request_id: RequestId::Integer(12),
        command: AppClientCommand::RequestConversationHistoryPage {
            conversation_id: "default".to_string(),
            before_turn_id: Some("turn-9".to_string()),
            limit: 25,
        },
        context: None,
    };
    let rpc = JsonRpcMessage::from(envelope);
    let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");
    match parsed.command {
        AppClientCommand::RequestConversationHistoryPage {
            conversation_id,
            before_turn_id,
            limit,
        } => {
            assert_eq!(conversation_id, "default");
            assert_eq!(before_turn_id.as_deref(), Some("turn-9"));
            assert_eq!(limit, 25);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn list_online_nodes_roundtrips_from_jsonrpc_request() {
    let envelope = AppClientCommandEnvelope {
        request_id: RequestId::Integer(13),
        command: AppClientCommand::ListOnlineNodes,
        context: None,
    };
    let rpc = JsonRpcMessage::from(envelope);
    let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");
    assert_eq!(parsed.request_id, RequestId::Integer(13));
    assert!(matches!(parsed.command, AppClientCommand::ListOnlineNodes));
}

#[test]
fn select_target_node_roundtrips_from_jsonrpc_request() {
    let envelope = AppClientCommandEnvelope {
        request_id: RequestId::Integer(14),
        command: AppClientCommand::SelectTargetNode {
            node_id: "node-a".to_string(),
        },
        context: None,
    };
    let rpc = JsonRpcMessage::from(envelope);
    let parsed = AppClientCommandEnvelope::try_from(rpc).expect("command should parse");
    match parsed.command {
        AppClientCommand::SelectTargetNode { node_id } => {
            assert_eq!(node_id, "node-a");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn online_node_list_roundtrips_through_jsonrpc_notification() {
    let message = AppServerMessageEnvelope {
        message: AppServerMessage::Notification(AppServerNotification::OnlineNodeList {
            conversation_id: "default".to_string(),
            nodes: vec![OnlineNodeSummary {
                node_id: "node-a".to_string(),
                display_name: "Node A".to_string(),
                labels: vec!["gpu".to_string()],
                version: "0.1.0".to_string(),
                online: true,
            }],
        }),
        event_seq: None,
    };

    let JsonRpcMessage::Notification(notification) = JsonRpcMessage::from(message) else {
        panic!("expected notification");
    };
    assert_eq!(notification.method, "hub/node/list");

    let reparsed = AppServerMessageEnvelope::try_from(JsonRpcMessage::Notification(notification))
        .expect("reparse");
    match reparsed.message {
        AppServerMessage::Notification(AppServerNotification::OnlineNodeList {
            conversation_id,
            nodes,
        }) => {
            assert_eq!(conversation_id, "default");
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].node_id, "node-a");
            assert!(nodes[0].online);
        }
        other => panic!("unexpected notification: {other:?}"),
    }
}
