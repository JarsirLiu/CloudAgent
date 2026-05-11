use crate::message::InboundMessage;

pub fn build_session_key(message: &InboundMessage) -> String {
    let chat_type = normalize_chat_type(message.chat_type.as_deref());

    if chat_type == "dm" {
        let mut parts = vec![
            "agent".to_string(),
            "main".to_string(),
            message.platform.clone(),
            "dm".to_string(),
            message.chat_id.clone(),
        ];
        if let Some(thread_id) = &message.thread_id {
            parts.push(thread_id.clone());
        }
        return parts.join(":");
    }

    let mut parts = vec![
        "agent".to_string(),
        "main".to_string(),
        message.platform.clone(),
        chat_type.to_string(),
        message.chat_id.clone(),
    ];

    if let Some(thread_id) = &message.thread_id {
        parts.push(thread_id.clone());
    }

    if message.thread_id.is_none() {
        if let Some(union_id) = &message.sender_union_id {
            parts.push(union_id.clone());
        } else if let Some(user_id) = &message.sender_user_id {
            parts.push(user_id.clone());
        } else {
            parts.push(message.sender_open_id.clone());
        }
    }

    parts.join(":")
}

fn normalize_chat_type(chat_type: Option<&str>) -> &'static str {
    match chat_type.unwrap_or_default() {
        "p2p" | "dm" => "dm",
        "group" => "group",
        "channel" => "channel",
        "" => "unknown",
        _ => "group",
    }
}
