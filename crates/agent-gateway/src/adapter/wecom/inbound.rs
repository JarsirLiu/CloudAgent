use crate::message::{InboundMessage, ReplyContext};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WecomChatKind {
    Single,
    Group,
}

#[derive(Debug, Clone)]
pub struct WecomInboundEnvelope {
    pub chat_kind: WecomChatKind,
    pub chat_id: String,
    pub sender_user_id: String,
    pub message_id: String,
    pub text: String,
    pub image_urls: Vec<String>,
    pub mentioned: bool,
    pub reply_to_message_id: Option<String>,
    pub reply_req_id: Option<String>,
}

impl WecomInboundEnvelope {
    pub fn from_payload(payload: &Value) -> Option<Self> {
        let body = payload.get("body")?.as_object()?;
        let body_value = Value::Object(body.clone());

        let message_id = extract_string(&body_value, &["msgid", "msg_id", "message_id"])
            .or_else(|| extract_string(payload, &["headers.req_id"]))?;
        let chat_id = extract_string(&body_value, &["chatid", "chat_id", "conversation_id"])?;
        let sender_user_id = extract_string(
            &body_value,
            &[
                "from.userid",
                "from.user_id",
                "from_userid",
                "from_user_id",
                "userid",
                "user_id",
                "sender.user_id",
                "sender.userid",
            ],
        )?;
        let msg_type = extract_string(&body_value, &["msgtype", "msg_type"])
            .unwrap_or_else(|| "text".to_string());
        let text = extract_text(&body_value, &msg_type).unwrap_or_default();
        let image_urls = extract_image_urls(&body_value, &msg_type);
        let chat_kind = match extract_string(&body_value, &["chattype", "chat_type"])
            .unwrap_or_else(|| "single".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "group" => WecomChatKind::Group,
            _ => WecomChatKind::Single,
        };
        let mentioned = extract_bool(&body_value, &["mentioned", "is_at", "at_bot"])
            .unwrap_or(chat_kind == WecomChatKind::Single);
        let reply_to_message_id = extract_string(
            &body_value,
            &[
                "quote.msgid",
                "quote.message_id",
                "reply_to.msgid",
                "reply_to.message_id",
                "quoted.msgid",
            ],
        );
        let reply_req_id = extract_string(payload, &["headers.req_id"]);

        Some(Self {
            chat_kind,
            chat_id,
            sender_user_id,
            message_id,
            text,
            image_urls,
            mentioned,
            reply_to_message_id,
            reply_req_id,
        })
    }

    pub fn into_gateway_message(self, image_paths: Vec<String>) -> InboundMessage {
        let chat_type = match self.chat_kind {
            WecomChatKind::Single => Some("p2p".to_string()),
            WecomChatKind::Group => Some("group".to_string()),
        };
        InboundMessage {
            platform: "wecom".to_string(),
            tenant_key: None,
            chat_id: self.chat_id,
            chat_type,
            sender_open_id: self.sender_user_id.clone(),
            sender_user_id: Some(self.sender_user_id),
            sender_union_id: None,
            message_id: self.message_id,
            thread_id: None,
            text: self.text,
            image_paths,
            mentioned: self.mentioned,
            reply_context: self.reply_to_message_id.map(|message_id| ReplyContext {
                message_id,
                thread_id: None,
            }),
        }
    }
}

fn extract_text(body: &Value, msg_type: &str) -> Option<String> {
    match msg_type.to_ascii_lowercase().as_str() {
        "text" | "markdown" => extract_string(
            body,
            &[
                "text.content",
                "markdown.content",
                "content",
                "msg_content",
                "msg",
            ],
        ),
        "mixed" | "mixed_msg" => extract_string(
            body,
            &[
                "mixed.text.content",
                "text.content",
                "content",
                "msg_content",
            ],
        )
        .or_else(|| extract_mixed_text(body)),
        _ => extract_string(
            body,
            &[
                "text.content",
                "markdown.content",
                "content",
                "msg_content",
                "msg",
            ],
        ),
    }
    .map(|text| text.trim().to_string())
    .filter(|text| !text.is_empty())
}

fn extract_mixed_text(body: &Value) -> Option<String> {
    let items = body
        .get("mixed")
        .and_then(|mixed| mixed.get("msg_item"))
        .and_then(Value::as_array)?;
    let parts = items
        .iter()
        .filter(|item| {
            item.get("msgtype")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .eq_ignore_ascii_case("text")
        })
        .filter_map(|item| extract_string(item, &["text.content", "content"]))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn extract_image_urls(body: &Value, msg_type: &str) -> Vec<String> {
    let mut urls = Vec::new();
    match msg_type.to_ascii_lowercase().as_str() {
        "image" => {
            if let Some(url) = extract_string(body, &["image.url", "url"]) {
                urls.push(url);
            }
        }
        "mixed" | "mixed_msg" => {
            if let Some(items) = body
                .get("mixed")
                .and_then(|mixed| mixed.get("msg_item"))
                .and_then(Value::as_array)
            {
                for item in items {
                    if item
                        .get("msgtype")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .eq_ignore_ascii_case("image")
                        && let Some(url) = extract_string(item, &["image.url", "url"])
                    {
                        urls.push(url);
                    }
                }
            }
        }
        _ => {}
    }
    urls
}

fn extract_bool(value: &Value, paths: &[&str]) -> Option<bool> {
    paths.iter().find_map(|path| {
        find_path(value, path).and_then(|value| match value {
            Value::Bool(v) => Some(*v),
            Value::String(v) => match v.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" => Some(false),
                _ => None,
            },
            _ => None,
        })
    })
}

fn extract_string(value: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        find_path(value, path).and_then(|value| match value {
            Value::String(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
            Value::Number(v) => Some(v.to_string()),
            _ => None,
        })
    })
}

fn find_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::{WecomChatKind, WecomInboundEnvelope};
    use serde_json::json;

    #[test]
    fn parses_single_chat_payload() {
        let payload = json!({
            "headers": { "req_id": "req-1" },
            "body": {
                "msgid": "m1",
                "chatid": "chat1",
                "chattype": "single",
                "from": { "userid": "u1" },
                "msgtype": "text",
                "text": { "content": "hello" }
            }
        });
        let envelope = WecomInboundEnvelope::from_payload(&payload).expect("payload");
        assert_eq!(envelope.chat_kind, WecomChatKind::Single);
        assert_eq!(envelope.sender_user_id, "u1");
        assert_eq!(envelope.text, "hello");
        assert!(envelope.image_urls.is_empty());
        assert_eq!(envelope.reply_req_id.as_deref(), Some("req-1"));
    }

    #[test]
    fn parses_multiple_images_from_mixed_payload() {
        let payload = json!({
            "headers": { "req_id": "req-2" },
            "body": {
                "msgid": "m2",
                "chatid": "chat2",
                "chattype": "single",
                "from": { "userid": "u2" },
                "msgtype": "mixed",
                "mixed": {
                    "msg_item": [
                        { "msgtype": "text", "text": { "content": "look at these" } },
                        { "msgtype": "image", "image": { "url": "https://example.com/a.png" } },
                        { "msgtype": "image", "image": { "url": "https://example.com/b.jpg" } }
                    ]
                }
            }
        });
        let envelope = WecomInboundEnvelope::from_payload(&payload).expect("payload");
        assert_eq!(envelope.text, "look at these");
        assert_eq!(
            envelope.image_urls,
            vec![
                "https://example.com/a.png".to_string(),
                "https://example.com/b.jpg".to_string()
            ]
        );
    }
}
