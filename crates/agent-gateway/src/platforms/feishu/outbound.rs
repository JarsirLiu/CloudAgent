use crate::message::{OutboundMessage, ReplyContext};
use anyhow::{Context, Result};
use feishu_sdk::Client;
use serde_json::Value;
use serde_json::json;
use tracing::{info, warn};

use super::formatter::format_text_chunks;

pub async fn send_message(client: &Client, outbound: OutboundMessage) -> Result<Vec<String>> {
    let mut sent_message_ids = Vec::new();
    for chunk in format_text_chunks(&outbound.text, outbound.is_group_context) {
        if let Some(message_id) = send_raw_message(
            client,
            outbound.chat_id.clone(),
            outbound.reply_context.clone(),
            chunk.msg_type,
            chunk.content,
            &chunk.preview_text,
        )
        .await?
        {
            sent_message_ids.push(message_id);
        }
    }

    Ok(sent_message_ids)
}

pub async fn send_interactive_card(
    client: &Client,
    chat_id: String,
    reply_context: Option<ReplyContext>,
    card: Value,
    purpose: &str,
) -> Result<()> {
    let _ = send_raw_message(
        client,
        chat_id,
        reply_context,
        "interactive",
        card,
        &format!("interactive:{purpose}"),
    )
    .await?;
    Ok(())
}

async fn send_raw_message(
    client: &Client,
    chat_id: String,
    reply_context: Option<ReplyContext>,
    msg_type: &str,
    content: Value,
    preview_text: &str,
) -> Result<Option<String>> {
    if let Some(context) = &reply_context {
        let reply_in_thread = false;
        info!(
            chat_id = %chat_id,
            reply_to_message_id = %context.message_id,
            reply_in_thread,
            msg_type = %msg_type,
            text_preview = %preview_text,
            "feishu.outbound.reply.attempt"
        );
        match try_reply(client, context, msg_type, &content, reply_in_thread).await {
            Ok(Some(message_id)) => {
                info!(
                    chat_id = %chat_id,
                    reply_to_message_id = %context.message_id,
                    msg_type = %msg_type,
                    "feishu.outbound.reply.ok"
                );
                return Ok(Some(message_id));
            }
            Ok(None) => {}
            Err(error) => {
                warn!(
                    chat_id = %chat_id,
                    reply_to_message_id = %context.message_id,
                    msg_type = %msg_type,
                    ?error,
                    "feishu.outbound.reply.failed_fallback_to_create"
                );
            }
        }
    }

    info!(
        chat_id = %chat_id,
        msg_type = %msg_type,
        text_preview = %preview_text,
        "feishu.outbound.create.attempt"
    );
    let body = json!({
        "receive_id": chat_id.clone(),
        "msg_type": msg_type,
        "content": serde_json::to_string(&content)
            .context("failed to encode feishu message content")?,
    });

    let response = client
        .operation("im.v1.messages.create")
        .path_param("receive_id_type", "chat_id")
        .body_json(&body)
        .context("failed to build feishu create request")?
        .send()
        .await
        .context("failed to send feishu create request")?;

    if response.status != 200 {
        let body = String::from_utf8_lossy(&response.body);
        warn!(
            "feishu create returned status {}: {}",
            response.status, body
        );
    } else {
        info!(chat_id = %chat_id, msg_type = %msg_type, "feishu.outbound.create.ok");
    }
    Ok(parse_message_id_from_response(&response.body))
}

async fn try_reply(
    client: &Client,
    context: &ReplyContext,
    msg_type: &str,
    content: &Value,
    reply_in_thread: bool,
) -> Result<Option<String>> {
    let body = json!({
        "content": serde_json::to_string(content)
            .context("failed to encode feishu reply content")?,
        "msg_type": msg_type,
        "reply_in_thread": reply_in_thread,
    });

    let response = client
        .operation("im.v1.message.reply")
        .path_param("message_id", context.message_id.clone())
        .body_json(&body)
        .context("failed to build feishu reply request")?
        .send()
        .await
        .context("failed to send feishu reply request")?;

    if response.status == 200 {
        return Ok(parse_message_id_from_response(&response.body));
    }

    let body = String::from_utf8_lossy(&response.body);
    warn!(
        "feishu reply returned status {} for message {}: {}",
        response.status, context.message_id, body
    );
    Ok(None)
}

fn parse_message_id_from_response(body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    value
        .get("data")
        .and_then(|data| data.get("message_id"))
        .and_then(Value::as_str)
        .map(|message_id| message_id.to_string())
}
