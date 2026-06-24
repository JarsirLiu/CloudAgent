use super::build_turn_content_for_tests;
use crate::message::InboundMessage;

#[test]
fn build_turn_content_includes_text_and_images_in_order() {
    let message = InboundMessage {
        platform: "weixin".to_string(),
        tenant_key: None,
        chat_id: "chat-1".to_string(),
        chat_type: Some("p2p".to_string()),
        sender_open_id: "sender-open-id".to_string(),
        sender_user_id: Some("sender-user-id".to_string()),
        sender_union_id: None,
        message_id: "msg-1".to_string(),
        thread_id: Some("thread-1".to_string()),
        text: "hello".to_string(),
        image_paths: vec!["D:\\img-1.png".to_string()],
        mentioned: true,
        reply_context: None,
    };

    let content = build_turn_content_for_tests(&message);
    assert_eq!(content.len(), 2);
}
