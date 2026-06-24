use super::build_turn_content_for_tests;
use crate::adapter::runtime_shared::{
    event_name, notification_turn_id, parse_approval_command, render_request_prompt,
};
use crate::message::InboundMessage;

#[test]
fn build_turn_content_includes_text_and_images_in_order() {
    let message = InboundMessage {
        platform: "feishu".to_string(),
        tenant_key: Some("tenant-1".to_string()),
        chat_id: "chat-1".to_string(),
        chat_type: Some("p2p".to_string()),
        sender_open_id: "sender-open-id".to_string(),
        sender_user_id: Some("sender-user-id".to_string()),
        sender_union_id: Some("sender-union-id".to_string()),
        message_id: "msg-1".to_string(),
        thread_id: None,
        text: "hello".to_string(),
        image_paths: vec!["D:\\img-1.png".to_string()],
        mentioned: true,
        reply_context: None,
    };

    let content = build_turn_content_for_tests(&message);
    assert_eq!(content.len(), 2);
}

#[test]
fn shared_gateway_helpers_keep_runtime_semantics_stable() {
    let decision = parse_approval_command("/approve").expect("decision");
    assert!(decision.label().contains("approved"));

    let request = agent_protocol::AppServerRequest::ServerRequest {
        request_id: agent_protocol::RequestId::Integer(1),
        conversation_id: "conv-1".to_string(),
        request: agent_core::ServerRequest::CommandApproval {
            request: agent_core::CommandApprovalRequest {
                turn_id: "turn-1".to_string(),
                tool_call_id: "call-1".to_string(),
                tool_name: "exec_command".to_string(),
                reason: "need approval".to_string(),
                command_preview: "echo hello".to_string(),
            },
        },
    };
    let prompt = render_request_prompt(&request);
    assert!(prompt.contains("命令执行"));

    assert_eq!(
        notification_turn_id(&agent_protocol::AppServerNotification::TurnCompleted {
            conversation_id: "conv-1".to_string(),
            turn_id: "turn-1".to_string(),
        }),
        Some("turn-1")
    );
    assert_eq!(
        event_name(&agent_app_server_client::AppServerEvent::Message(
            agent_protocol::AppServerMessage::Notification(
                agent_protocol::AppServerNotification::TurnCompleted {
                    conversation_id: "conv-1".to_string(),
                    turn_id: "turn-1".to_string(),
                }
            )
        )),
        "turn_completed"
    );
}
