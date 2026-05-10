use crate::GatewayMessage;
use agent_core::{AttachmentRef, InputItem};
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeishuInboundMessage {
    Text {
        conversation_id: String,
        sender_id: String,
        text: String,
    },
    Image {
        conversation_id: String,
        sender_id: String,
        image_key: String,
    },
    File {
        conversation_id: String,
        sender_id: String,
        file_key: String,
        file_name: String,
    },
}

impl FeishuInboundMessage {
    pub(crate) fn from_event(
        sender_open_id: String,
        chat_id: String,
        message_type: String,
        content: String,
        root_id: Option<String>,
    ) -> Result<Option<Self>> {
        let conversation_id = if chat_id.starts_with("oc_") {
            if let Some(root_id) = root_id {
                format!("feishu:chat:{chat_id}:thread:{root_id}")
            } else {
                format!("feishu:chat:{chat_id}")
            }
        } else {
            format!("feishu:p2p:{sender_open_id}")
        };

        match message_type.as_str() {
            "text" => {
                let payload: FeishuTextContent =
                    serde_json::from_str(&content).context("parse feishu text content")?;
                Ok(Some(Self::Text {
                    conversation_id,
                    sender_id: sender_open_id,
                    text: payload.text,
                }))
            }
            "image" => {
                let payload: FeishuImageContent =
                    serde_json::from_str(&content).context("parse feishu image content")?;
                Ok(Some(Self::Image {
                    conversation_id,
                    sender_id: sender_open_id,
                    image_key: payload.image_key,
                }))
            }
            "file" => {
                let payload: FeishuFileContent =
                    serde_json::from_str(&content).context("parse feishu file content")?;
                Ok(Some(Self::File {
                    conversation_id,
                    sender_id: sender_open_id,
                    file_key: payload.file_key,
                    file_name: payload.file_name,
                }))
            }
            _ => Ok(None),
        }
    }

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
                image_key,
            } => GatewayMessage::new(
                conversation_id,
                sender_id,
                vec![InputItem::Image {
                    source: AttachmentRef::RemoteUrl {
                        url: format!("feishu://image/{image_key}"),
                    },
                    detail: None,
                    alt: Some("feishu image".to_string()),
                }],
            ),
            Self::File {
                conversation_id,
                sender_id,
                file_key,
                file_name,
            } => GatewayMessage::new(
                conversation_id,
                sender_id,
                vec![InputItem::File {
                    source: AttachmentRef::RemoteUrl {
                        url: format!("feishu://file/{file_key}"),
                    },
                    mime_type: None,
                    name: Some(file_name),
                }],
            ),
        }
    }
}

#[derive(Deserialize)]
struct FeishuTextContent {
    text: String,
}

#[derive(Deserialize)]
struct FeishuImageContent {
    image_key: String,
}

#[derive(Deserialize)]
struct FeishuFileContent {
    file_key: String,
    file_name: String,
}
