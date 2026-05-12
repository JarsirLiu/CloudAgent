use crate::message::InboundMessage;
use serde::Deserialize;
use serde_json::Value;

use super::reply_context::build_reply_context;
use super::types::{
    FeishuBotIdentity, FeishuMention, FeishuMessageEnvelope, NormalizedFeishuMessage,
};

#[derive(Debug, Deserialize)]
struct FeishuTextContent {
    text: Option<String>,
}

pub fn normalize_inbound(
    envelope: FeishuMessageEnvelope,
    bot: &FeishuBotIdentity,
) -> Option<InboundMessage> {
    let sender = envelope.sender.sender_id.clone()?;
    let sender_open_id = sender.open_id?;
    let chat_id = envelope.message.chat_id.clone()?;
    let message_id = envelope.message.message_id.clone()?;
    let mentions = envelope.message.mentions.clone().unwrap_or_default();

    let normalized = normalize_content(
        envelope.message.message_type.as_deref().unwrap_or_default(),
        envelope.message.content.as_deref().unwrap_or_default(),
        &mentions,
        bot,
    );

    Some(InboundMessage {
        platform: "feishu".to_string(),
        tenant_key: envelope.sender.tenant_key.clone(),
        chat_id,
        chat_type: envelope.message.chat_type.clone(),
        sender_open_id,
        sender_user_id: sender.user_id,
        sender_union_id: sender.union_id,
        message_id,
        thread_id: envelope
            .message
            .root_id
            .clone()
            .or_else(|| envelope.message.parent_id.clone()),
        text: normalized.text.trim().to_string(),
        image_paths: Vec::new(),
        mentioned: normalized.mentioned,
        reply_context: build_reply_context(&envelope),
    })
}

fn normalize_content(
    message_type: &str,
    raw_content: &str,
    mentions: &[FeishuMention],
    bot: &FeishuBotIdentity,
) -> NormalizedFeishuMessage {
    let message_type = message_type.trim().to_ascii_lowercase();
    let mentioned = mentions_bot(mentions, bot);

    let text = match message_type.as_str() {
        "text" => extract_text(raw_content),
        "post" => extract_post_text(raw_content),
        "image" => "[Image]".to_string(),
        "file" => extract_file_label(raw_content),
        _ => extract_text(raw_content),
    };

    let mut normalized = text;
    for mention in mentions {
        if let Some(key) = &mention.key {
            let replacement = mention
                .name
                .as_deref()
                .map(|name| format!("@{name}"))
                .unwrap_or_default();
            normalized = normalized.replace(key, &replacement);
        }
    }

    NormalizedFeishuMessage {
        text: normalized,
        mentioned,
    }
}

fn mentions_bot(mentions: &[FeishuMention], bot: &FeishuBotIdentity) -> bool {
    if mentions.is_empty() {
        return false;
    }

    if bot.open_id.trim().is_empty() && bot.name.trim().is_empty() {
        return true;
    }

    mentions.iter().any(|mention| {
        let open_id_matches = mention
            .id
            .as_ref()
            .and_then(|id| id.open_id.as_deref())
            .map(|open_id| !bot.open_id.is_empty() && open_id == bot.open_id)
            .unwrap_or(false);

        let name_matches = mention
            .name
            .as_deref()
            .map(|name| !bot.name.is_empty() && name == bot.name)
            .unwrap_or(false);

        open_id_matches || name_matches
    })
}

fn extract_text(raw_content: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<FeishuTextContent>(raw_content)
        && let Some(text) = parsed.text
    {
        return text;
    }

    if let Ok(value) = serde_json::from_str::<Value>(raw_content)
        && let Some(text) = value.get("text").and_then(Value::as_str)
    {
        return text.to_string();
    }

    raw_content.to_string()
}

fn extract_post_text(raw_content: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(raw_content) else {
        return raw_content.to_string();
    };

    let mut parts = Vec::new();
    collect_text(&value, &mut parts);
    let text = parts.join("\n").trim().to_string();
    if text.is_empty() {
        "[Rich text message]".to_string()
    } else {
        text
    }
}

fn collect_text(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text(item, parts);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
            for item in map.values() {
                collect_text(item, parts);
            }
        }
        _ => {}
    }
}

fn extract_file_label(raw_content: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(raw_content) else {
        return "[Attachment]".to_string();
    };

    value
        .get("file_name")
        .and_then(Value::as_str)
        .map(|name| format!("[Attachment] {name}"))
        .unwrap_or_else(|| "[Attachment]".to_string())
}
