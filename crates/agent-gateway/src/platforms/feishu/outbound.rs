use crate::message::{OutboundMessage, ReplyContext};
use anyhow::{Context, Result};
use feishu_sdk::Client;
use serde_json::Value;
use serde_json::json;
use tracing::{info, warn};

use super::formatter::format_text_chunks;

pub async fn send_message(client: &Client, outbound: OutboundMessage) -> Result<()> {
    for chunk in format_text_chunks(&outbound.text) {
        send_raw_message(
            client,
            outbound.chat_id.clone(),
            outbound.reply_context.clone(),
            chunk.msg_type,
            chunk.content,
            &chunk.preview_text,
        )
        .await?;
    }

    Ok(())
}

pub async fn send_interactive_card(
    client: &Client,
    chat_id: String,
    reply_context: Option<ReplyContext>,
    card: Value,
    purpose: &str,
) -> Result<()> {
    send_raw_message(
        client,
        chat_id,
        reply_context,
        "interactive",
        card,
        &format!("interactive:{purpose}"),
    )
    .await
}

async fn send_raw_message(
    client: &Client,
    chat_id: String,
    reply_context: Option<ReplyContext>,
    msg_type: &str,
    content: Value,
    preview_text: &str,
) -> Result<()> {
    if let Some(context) = &reply_context {
        info!(
            chat_id = %chat_id,
            reply_to_message_id = %context.message_id,
            reply_in_thread = context.thread_id.is_some(),
            msg_type = %msg_type,
            text_preview = %preview_text,
            "feishu.outbound.reply.attempt"
        );
        if try_reply(client, context, msg_type, &content).await? {
            info!(
                chat_id = %chat_id,
                reply_to_message_id = %context.message_id,
                msg_type = %msg_type,
                "feishu.outbound.reply.ok"
            );
            return Ok(());
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
    Ok(())
}

async fn try_reply(
    client: &Client,
    context: &ReplyContext,
    msg_type: &str,
    content: &Value,
) -> Result<bool> {
    let body = json!({
        "content": serde_json::to_string(content)
            .context("failed to encode feishu reply content")?,
        "msg_type": msg_type,
        "reply_in_thread": context.thread_id.is_some(),
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
        return Ok(true);
    }

    let body = String::from_utf8_lossy(&response.body);
    warn!(
        "feishu reply returned status {} for message {}: {}",
        response.status, context.message_id, body
    );
    Ok(false)
}
