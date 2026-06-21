    use super::{ChatApiMessage, ProviderMessage, tool_message_content};
    use agent_core::{AttachmentRef, ImageDetail, InputItem};
    use serde_json::json;

    #[test]
    fn tool_messages_forward_content_verbatim() {
        let filtered = "[rtk:generic]\nCommand summary\n- listed workspace files";
        let rendered = tool_message_content("exec_command", filtered);

        assert_eq!(rendered, filtered);
    }

    #[test]
    fn user_messages_encode_text_and_image_parts_for_openai_wire() {
        let message = ChatApiMessage::from_message(&ProviderMessage::User {
            content: vec![
                InputItem::Text {
                    text: "describe this".to_string(),
                },
                InputItem::Image {
                    source: AttachmentRef::RemoteUrl {
                        url: "https://example.com/diagram.png".to_string(),
                    },
                    detail: Some(ImageDetail::High),
                    alt: Some("diagram".to_string()),
                },
            ],
        });

        let value = serde_json::to_value(message).expect("serialize user message");
        assert_eq!(
            value,
            json!({
                "role": "user",
                "content": [
                    { "type": "text", "text": "describe this" },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.com/diagram.png",
                            "detail": "high"
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn local_path_images_fall_back_to_alt_text_until_materialized() {
        let message = ChatApiMessage::from_message(&ProviderMessage::User {
            content: vec![InputItem::Image {
                source: AttachmentRef::LocalPath {
                    path: "C:\\tmp\\plan.png".to_string(),
                },
                detail: Some(ImageDetail::Low),
                alt: Some("plan".to_string()),
            }],
        });

        let value = serde_json::to_value(message).expect("serialize user message");
        assert_eq!(
            value,
            json!({
                "role": "user",
                "content": [
                    { "type": "text", "text": "[image unavailable: plan]" }
                ]
            })
        );
    }

    #[test]
    fn assistant_messages_include_reasoning_content_when_present() {
        let message = ChatApiMessage::from_message(&ProviderMessage::Assistant {
            content: Some("answer".to_string()),
            reasoning: Some("hidden chain".to_string()),
            tool_calls: Vec::new(),
        });

        let value = serde_json::to_value(message).expect("serialize assistant message");
        assert_eq!(
            value,
            json!({
                "role": "assistant",
                "content": "answer",
                "reasoning_content": "hidden chain"
            })
        );
    }
