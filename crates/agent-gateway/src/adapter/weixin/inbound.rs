use crate::message::InboundMessage;
use serde_json::Value;

const ITEM_TEXT: i64 = 1;

#[derive(Debug, Clone)]
pub struct WeixinInboundEnvelope {
    pub chat_id: String,
    pub chat_type: String,
    pub sender_user_id: String,
    pub message_id: Option<String>,
    pub text: String,
    pub context_token: Option<String>,
}

impl WeixinInboundEnvelope {
    pub fn from_message(message: &Value, account_id: &str) -> Option<Self> {
        let sender_user_id = extract_string(message, &["from_user_id"])?;
        let message_id = extract_string(message, &["message_id"]);
        let room_id = extract_string(message, &["room_id", "chat_room_id"]);
        let to_user_id = extract_string(message, &["to_user_id"]).unwrap_or_default();
        let (chat_type, chat_id) = if let Some(room_id) = room_id.filter(|value| !value.is_empty())
        {
            ("group".to_string(), room_id)
        } else if !to_user_id.is_empty()
            && !account_id.trim().is_empty()
            && to_user_id != account_id
        {
            ("group".to_string(), to_user_id)
        } else {
            ("dm".to_string(), sender_user_id.clone())
        };
        let text = extract_text(message)?;
        Some(Self {
            chat_id,
            chat_type,
            sender_user_id,
            message_id,
            text,
            context_token: extract_string(message, &["context_token"]),
        })
    }

    pub fn into_gateway_message(self) -> InboundMessage {
        InboundMessage {
            platform: "weixin".to_string(),
            tenant_key: None,
            chat_id: self.chat_id,
            chat_type: Some(self.chat_type),
            sender_open_id: self.sender_user_id.clone(),
            sender_user_id: Some(self.sender_user_id),
            sender_union_id: None,
            message_id: self.message_id.unwrap_or_default(),
            thread_id: None,
            text: self.text,
            image_paths: Vec::new(),
            mentioned: true,
            reply_context: None,
        }
    }
}

fn extract_text(message: &Value) -> Option<String> {
    let items = message.get("item_list")?.as_array()?;
    let parts = items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_i64) == Some(ITEM_TEXT))
        .filter_map(|item| {
            item.get("text_item")
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn extract_string(value: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        let mut current = value;
        for part in path.split('.') {
            current = current.get(part)?;
        }
        match current {
            Value::String(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
            Value::Number(v) => Some(v.to_string()),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::WeixinInboundEnvelope;
    use serde_json::json;

    #[test]
    fn parses_dm_text_message() {
        let payload = json!({
            "from_user_id": "wxid_user1",
            "message_id": "m1",
            "item_list": [
                { "type": 1, "text_item": { "text": "hello" } }
            ]
        });
        let envelope =
            WeixinInboundEnvelope::from_message(&payload, "bot-account").expect("envelope");
        assert_eq!(envelope.chat_type, "dm");
        assert_eq!(envelope.chat_id, "wxid_user1");
        assert_eq!(envelope.message_id.as_deref(), Some("m1"));
        assert_eq!(envelope.text, "hello");
    }

    #[test]
    fn leaves_message_id_empty_when_missing() {
        let payload = json!({
            "from_user_id": "wxid_user1",
            "item_list": [
                { "type": 1, "text_item": { "text": "hello" } }
            ]
        });
        let envelope =
            WeixinInboundEnvelope::from_message(&payload, "bot-account").expect("envelope");
        assert!(envelope.message_id.is_none());
    }
}
