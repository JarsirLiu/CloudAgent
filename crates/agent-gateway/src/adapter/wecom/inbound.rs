use crate::GatewayMessage;
use agent_core::{AttachmentRef, InputItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WecomInboundMessage {
    Text {
        conversation_id: String,
        sender_id: String,
        text: String,
    },
    Image {
        conversation_id: String,
        sender_id: String,
        media_url: String,
    },
    File {
        conversation_id: String,
        sender_id: String,
        media_url: String,
        file_name: String,
    },
}

impl WecomInboundMessage {
    pub fn into_gateway_message(self) -> GatewayMessage {
        match self {
            Self::Text {
                conversation_id,
                sender_id,
                text,
            } => GatewayMessage::new(conversation_id, sender_id, vec![InputItem::Text { text }]),
            Self::Image {
                conversation_id,
                sender_id,
                media_url,
            } => GatewayMessage::new(
                conversation_id,
                sender_id,
                vec![InputItem::Image {
                    source: AttachmentRef::RemoteUrl { url: media_url },
                    detail: None,
                    alt: Some("wecom image".to_string()),
                }],
            ),
            Self::File {
                conversation_id,
                sender_id,
                media_url,
                file_name,
            } => GatewayMessage::new(
                conversation_id,
                sender_id,
                vec![InputItem::File {
                    source: AttachmentRef::RemoteUrl { url: media_url },
                    mime_type: None,
                    name: Some(file_name),
                }],
            ),
        }
    }
}
