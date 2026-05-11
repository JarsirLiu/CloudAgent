use super::config::WeixinAdapterConfig;
use super::inbound::WeixinInboundEnvelope;
use super::outbound::{WeixinOutboundMessage, WeixinOutboundRenderer};
use crate::gateway_event::GatewayEvent;
use crate::platform::{MessageHandler, PlatformAdapter};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{info, warn};
use uuid::Uuid;

const API_TIMEOUT_SECS: u64 = 45;
const INBOUND_DEDUP_LIMIT: usize = 1024;
const RETRY_DELAY: Duration = Duration::from_secs(2);
const BACKOFF_DELAY: Duration = Duration::from_secs(15);
const MAX_MESSAGE_LENGTH: usize = 2000;
const WEIXIN_COPY_LINE_WIDTH: usize = 72;
const CONFIG_TIMEOUT_SECS: u64 = 10;
const CHANNEL_VERSION: &str = "2.2.0";
const TYPING_START: i64 = 1;
const TYPING_STOP: i64 = 2;
const SESSION_EXPIRED_ERRCODE: i64 = -14;
const RATE_LIMIT_ERRCODE: i64 = -2;

#[derive(Clone)]
pub struct WeixinAdapter {
    config: WeixinAdapterConfig,
    http: reqwest::Client,
    renderer: Arc<Mutex<WeixinOutboundRenderer>>,
    seen_messages: Arc<Mutex<SeenMessages>>,
    context_tokens: Arc<Mutex<HashMap<String, String>>>,
    typing_tickets: Arc<Mutex<HashMap<String, String>>>,
    typing_active: Arc<Mutex<HashSet<String>>>,
}

impl WeixinAdapter {
    pub fn new(config: WeixinAdapterConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(API_TIMEOUT_SECS))
                .build()
                .context("failed to build weixin http client")?,
            renderer: Arc::new(Mutex::new(WeixinOutboundRenderer::default())),
            seen_messages: Arc::new(Mutex::new(SeenMessages::default())),
            context_tokens: Arc::new(Mutex::new(HashMap::new())),
            typing_tickets: Arc::new(Mutex::new(HashMap::new())),
            typing_active: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    async fn poll_updates(&self, sync_buf: &str) -> Result<Value> {
        self.post(
            "ilink/bot/getupdates",
            json!({ "get_updates_buf": sync_buf }),
        )
        .await
    }

    async fn send_text(&self, message: WeixinOutboundMessage) -> Result<()> {
        if message.content.trim().is_empty() {
            return Ok(());
        }
        let formatted = format_message_for_weixin(&message.content);
        let chunks = split_text_for_weixin_delivery(&formatted, MAX_MESSAGE_LENGTH);
        info!(
            chat_id = %message.chat_id,
            original_chars = message.content.chars().count(),
            formatted_chars = formatted.chars().count(),
            chunk_count = chunks.len(),
            preview = %preview(&formatted, 120),
            "weixin.send_text.prepared"
        );
        for chunk in chunks {
            self.send_text_chunk(&message.chat_id, chunk).await?;
        }
        Ok(())
    }

    async fn send_text_chunk(&self, chat_id: &str, content: String) -> Result<()> {
        let mut context_token = self.context_tokens.lock().await.get(chat_id).cloned();
        let mut retried_without_token = false;
        for attempt in 0..=2 {
            let client_id = format!("cloudagent-weixin-{}", Uuid::new_v4().simple());
            info!(
                chat_id,
                client_id,
                attempt,
                chars = content.chars().count(),
                has_context_token = context_token.is_some(),
                preview = %preview(&content, 120),
                "weixin.sendmessage.attempt"
            );
            let mut payload = json!({
                "msg": {
                    "from_user_id": "",
                    "to_user_id": chat_id,
                    "client_id": client_id,
                    "message_type": 2,
                    "message_state": 2,
                    "item_list": [
                        {
                            "type": 1,
                            "text_item": { "text": content }
                        }
                    ]
                }
            });
            if let Some(context_token) = context_token.as_ref()
                && let Some(msg) = payload.get_mut("msg").and_then(Value::as_object_mut)
            {
                msg.insert(
                    "context_token".to_string(),
                    Value::String(context_token.clone()),
                );
            }
            let response = self.post("ilink/bot/sendmessage", payload).await?;
            let ret = response.get("ret").and_then(Value::as_i64).unwrap_or(0);
            let errcode = response.get("errcode").and_then(Value::as_i64).unwrap_or(0);
            info!(
                chat_id,
                client_id,
                ret,
                errcode,
                response = %response,
                "weixin.sendmessage.response"
            );
            if ret == 0 && errcode == 0 {
                return Ok(());
            }
            if is_stale_session_ret(ret, errcode, response.get("errmsg").and_then(Value::as_str))
                && !retried_without_token
                && context_token.is_some()
            {
                retried_without_token = true;
                context_token = None;
                self.context_tokens.lock().await.remove(chat_id);
                warn!(
                    chat_id,
                    client_id,
                    "weixin.sendmessage.retry_without_context_token"
                );
                continue;
            }
            if (ret == RATE_LIMIT_ERRCODE || errcode == RATE_LIMIT_ERRCODE) && attempt < 2 {
                sleep(Duration::from_secs(3 * (attempt + 1))).await;
                continue;
            }
            anyhow::bail!("weixin sendmessage failed: {response}");
        }
        Ok(())
    }

    async fn ensure_typing_ticket(&self, chat_id: &str) -> Result<Option<String>> {
        if let Some(ticket) = self.typing_tickets.lock().await.get(chat_id).cloned() {
            return Ok(Some(ticket));
        }
        let context_token = self.context_tokens.lock().await.get(chat_id).cloned();
        let mut payload = json!({ "ilink_user_id": chat_id });
        if let Some(context_token) = context_token
            && let Some(map) = payload.as_object_mut()
        {
            map.insert("context_token".to_string(), Value::String(context_token));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(CONFIG_TIMEOUT_SECS))
            .build()
            .context("failed to build weixin config client")?;
        let url = format!("{}/{}", self.config.base_url.trim_end_matches('/'), "ilink/bot/getconfig");
        let response = client
            .post(url)
            .bearer_auth(self.config.token.trim())
            .header("AuthorizationType", "ilink_bot_token")
            .header("iLink-App-Id", "bot")
            .header("iLink-App-ClientVersion", "131584")
            .json(&payload)
            .send()
            .await
            .context("weixin getconfig failed")?
            .error_for_status()
            .context("weixin getconfig returned error status")?
            .json::<Value>()
            .await
            .context("failed to decode weixin getconfig response")?;
        let ticket = response
            .get("typing_ticket")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(ticket) = ticket.clone() {
            self.typing_tickets
                .lock()
                .await
                .insert(chat_id.to_string(), ticket);
        }
        Ok(ticket)
    }

    async fn send_typing_status(&self, chat_id: &str, status: i64) -> Result<()> {
        let Some(ticket) = self.ensure_typing_ticket(chat_id).await? else {
            return Ok(());
        };
        let payload = json!({
            "ilink_user_id": chat_id,
            "typing_ticket": ticket,
            "status": status
        });
        let response = self.post("ilink/bot/sendtyping", payload).await?;
        let ret = response.get("ret").and_then(Value::as_i64).unwrap_or(0);
        let errcode = response.get("errcode").and_then(Value::as_i64).unwrap_or(0);
        if ret != 0 || errcode != 0 {
            anyhow::bail!("weixin sendtyping failed: {response}");
        }
        Ok(())
    }

    async fn start_typing_if_needed(&self, chat_id: &str) {
        let mut active = self.typing_active.lock().await;
        if active.contains(chat_id) {
            return;
        }
        if let Err(error) = self.send_typing_status(chat_id, TYPING_START).await {
            warn!(?error, chat_id, "weixin.typing.start_failed");
            return;
        }
        active.insert(chat_id.to_string());
    }

    async fn stop_typing_if_needed(&self, chat_id: &str) {
        let mut active = self.typing_active.lock().await;
        if !active.remove(chat_id) {
            return;
        }
        if let Err(error) = self.send_typing_status(chat_id, TYPING_STOP).await {
            warn!(?error, chat_id, "weixin.typing.stop_failed");
        }
    }

    async fn post(&self, endpoint: &str, payload: Value) -> Result<Value> {
        let url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );
        let body = with_base_info(payload);
        let response = self
            .http
            .post(url)
            .bearer_auth(self.config.token.trim())
            .header("AuthorizationType", "ilink_bot_token")
            .header("X-WECHAT-UIN", random_wechat_uin())
            .header("iLink-App-Id", "bot")
            .header("iLink-App-ClientVersion", "131584")
            .json(&body)
            .send()
            .await
            .context("weixin request failed")?
            .error_for_status()
            .context("weixin request returned error status")?;
        response
            .json::<Value>()
            .await
            .context("failed to decode weixin response")
    }

    async fn handle_update(&self, message: Value, handler: Arc<dyn MessageHandler>) -> Result<()> {
        let Some(envelope) = WeixinInboundEnvelope::from_message(&message, &self.config.account_id) else {
            return Ok(());
        };
        if self
            .seen_messages
            .lock()
            .await
            .is_duplicate(&envelope)
        {
            return Ok(());
        }
        if let Some(context_token) = envelope.context_token.clone() {
            self.context_tokens
                .lock()
                .await
                .insert(envelope.chat_id.clone(), context_token);
        }
        if envelope.chat_type == "dm" {
            let _ = self.ensure_typing_ticket(&envelope.chat_id).await;
        }
        let inbound = envelope.into_gateway_message();
        if inbound.text.trim().is_empty() {
            return Ok(());
        }
        if handler.try_handle_session_command(&inbound).await? {
            return Ok(());
        }
        handler.handle_message(inbound).await
    }
}

fn split_text_for_weixin_delivery(content: &str, max_length: usize) -> Vec<String> {
    let content = content.trim();
    if content.is_empty() {
        return Vec::new();
    }
    if content.chars().count() <= max_length {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for block in split_markdown_blocks(content) {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        let candidate = if current.is_empty() {
            block.to_string()
        } else {
            format!("{current}\n\n{block}")
        };
        if candidate.chars().count() <= max_length {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if block.chars().count() <= max_length {
            current = block.to_string();
            continue;
        }
        split_hard(block, max_length, &mut chunks);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn with_base_info(payload: Value) -> Value {
    match payload {
        Value::Object(mut map) => {
            map.insert(
                "base_info".to_string(),
                json!({
                    "channel_version": CHANNEL_VERSION,
                }),
            );
            Value::Object(map)
        }
        other => other,
    }
}

fn random_wechat_uin() -> String {
    use base64::Engine as _;
    use rand::RngCore as _;

    let mut bytes = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut bytes);
    let value = u32::from_be_bytes(bytes);
    base64::engine::general_purpose::STANDARD.encode(value.to_string())
}

fn is_stale_session_ret(ret: i64, errcode: i64, errmsg: Option<&str>) -> bool {
    if ret == SESSION_EXPIRED_ERRCODE || errcode == SESSION_EXPIRED_ERRCODE {
        return true;
    }
    if ret != RATE_LIMIT_ERRCODE && errcode != RATE_LIMIT_ERRCODE {
        return false;
    }
    matches!(errmsg, Some(message) if message.trim().eq_ignore_ascii_case("unknown error"))
}

fn format_message_for_weixin(content: &str) -> String {
    wrap_copy_friendly_lines_for_weixin(&normalize_markdown_blocks(content))
}

fn normalize_markdown_blocks(content: &str) -> String {
    let mut result = Vec::new();
    let mut in_code_block = false;
    let mut blank_run = 0usize;

    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        if is_fence_line(line) {
            in_code_block = !in_code_block;
            result.push(line.to_string());
            blank_run = 0;
            continue;
        }
        if in_code_block {
            result.push(line.to_string());
            continue;
        }
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                result.push(String::new());
            }
            continue;
        }
        blank_run = 0;
        result.push(line.to_string());
    }

    result.join("\n").trim().to_string()
}

fn wrap_copy_friendly_lines_for_weixin(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }

    let mut wrapped = Vec::new();
    let mut in_code_block = false;
    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        let stripped = line.trim();
        if is_fence_line(stripped) {
            in_code_block = !in_code_block;
            wrapped.push(line.to_string());
            continue;
        }
        if in_code_block
            || line.chars().count() <= WEIXIN_COPY_LINE_WIDTH
            || stripped.is_empty()
            || stripped.starts_with('|')
            || is_table_rule_line(stripped)
        {
            wrapped.push(line.to_string());
            continue;
        }
        wrapped.extend(wrap_line_preserving_words(line, WEIXIN_COPY_LINE_WIDTH));
    }
    wrapped.join("\n").trim().to_string()
}

fn split_markdown_blocks(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }

    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut in_code_block = false;

    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        if is_fence_line(line) {
            if !in_code_block && !current.is_empty() {
                blocks.push(current.join("\n").trim().to_string());
                current.clear();
            }
            current.push(line.to_string());
            in_code_block = !in_code_block;
            if !in_code_block {
                blocks.push(current.join("\n").trim().to_string());
                current.clear();
            }
            continue;
        }
        if in_code_block {
            current.push(line.to_string());
            continue;
        }
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(current.join("\n").trim().to_string());
                current.clear();
            }
            continue;
        }
        current.push(line.to_string());
    }

    if !current.is_empty() {
        blocks.push(current.join("\n").trim().to_string());
    }
    blocks.into_iter().filter(|b| !b.is_empty()).collect()
}

fn wrap_line_preserving_words(line: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in line.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if candidate.chars().count() <= width {
            current = candidate;
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            if word.chars().count() <= width {
                current = word.to_string();
            } else {
                let mut hard = String::new();
                for ch in word.chars() {
                    hard.push(ch);
                    if hard.chars().count() >= width {
                        out.push(std::mem::take(&mut hard));
                    }
                }
                current = hard;
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        vec![line.to_string()]
    } else {
        out
    }
}

fn is_fence_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn is_table_rule_line(line: &str) -> bool {
    !line.is_empty()
        && line
            .chars()
            .all(|ch| matches!(ch, ':' | '-' | '|' | ' '))
}

fn split_hard(block: &str, max_length: usize, out: &mut Vec<String>) {
    let mut current = String::new();
    for ch in block.chars() {
        current.push(ch);
        if current.chars().count() >= max_length {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SeenMessages, format_message_for_weixin, is_stale_session_ret, random_wechat_uin,
        split_text_for_weixin_delivery,
        with_base_info,
    };
    use serde_json::json;
    use crate::adapter::weixin::inbound::WeixinInboundEnvelope;

    #[test]
    fn split_text_keeps_short_message_whole() {
        let chunks = split_text_for_weixin_delivery("hello\nworld", 2000);
        assert_eq!(chunks, vec!["hello\nworld".to_string()]);
    }

    #[test]
    fn split_text_packs_by_paragraph() {
        let text = format!("{}\n\n{}", "a".repeat(1500), "b".repeat(600));
        let chunks = split_text_for_weixin_delivery(&text, 2000);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains(&"a".repeat(100)));
        assert!(chunks[1].contains(&"b".repeat(100)));
    }

    #[test]
    fn format_message_collapses_extra_blank_lines() {
        let formatted = format_message_for_weixin("a\n\n\nb");
        assert_eq!(formatted, "a\n\nb");
    }

    #[test]
    fn dedup_uses_message_id_when_present() {
        let mut seen = SeenMessages::default();
        let first = envelope("chat-a", "user-a", Some("m1"), "hello");
        let second = envelope("chat-a", "user-a", Some("m1"), "different");

        assert!(!seen.is_duplicate(&first));
        assert!(seen.is_duplicate(&second));
    }

    #[test]
    fn dedup_allows_distinct_texts_without_message_id() {
        let mut seen = SeenMessages::default();
        let first = envelope("chat-a", "user-a", None, "hello");
        let duplicate = envelope("chat-a", "user-a", None, "hello");
        let distinct = envelope("chat-a", "user-a", None, "hello again");

        assert!(!seen.is_duplicate(&first));
        assert!(seen.is_duplicate(&duplicate));
        assert!(!seen.is_duplicate(&distinct));
    }

    #[test]
    fn stale_session_detects_expired_code() {
        assert!(is_stale_session_ret(-14, 0, None));
        assert!(is_stale_session_ret(0, -14, None));
    }

    #[test]
    fn stale_session_detects_unknown_error_rate_limit_shape() {
        assert!(is_stale_session_ret(-2, 0, Some("unknown error")));
        assert!(is_stale_session_ret(0, -2, Some("Unknown Error")));
        assert!(!is_stale_session_ret(-2, 0, Some("rate limited")));
    }

    #[test]
    fn wraps_payload_with_base_info() {
        let payload = with_base_info(json!({ "msg": { "hello": "world" } }));
        assert_eq!(payload["msg"]["hello"], "world");
        assert_eq!(payload["base_info"]["channel_version"], "2.2.0");
    }

    #[test]
    fn random_wechat_uin_is_base64_text() {
        let value = random_wechat_uin();
        assert!(!value.is_empty());
        assert!(value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=')));
    }

    fn envelope(
        chat_id: &str,
        sender_user_id: &str,
        message_id: Option<&str>,
        text: &str,
    ) -> WeixinInboundEnvelope {
        WeixinInboundEnvelope {
            chat_id: chat_id.to_string(),
            chat_type: "dm".to_string(),
            sender_user_id: sender_user_id.to_string(),
            message_id: message_id.map(str::to_string),
            text: text.to_string(),
            context_token: None,
        }
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for WeixinAdapter {
    fn platform_name(&self) -> &'static str {
        "weixin"
    }

    async fn run(self: Arc<Self>, handler: Arc<dyn MessageHandler>) -> Result<()> {
        info!(account_id = %self.config.account_id, base_url = %self.config.base_url, "weixin.long_poll.start");
        let mut sync_buf = String::new();
        let mut consecutive_failures = 0usize;
        loop {
            match self.poll_updates(&sync_buf).await {
                Ok(response) => {
                    consecutive_failures = 0;
                    let ret = response.get("ret").and_then(Value::as_i64).unwrap_or(0);
                    let errcode = response.get("errcode").and_then(Value::as_i64).unwrap_or(0);
                    if ret != 0 || errcode != 0 {
                        warn!(ret, errcode, response = %response, "weixin.long_poll.error");
                        sleep(BACKOFF_DELAY).await;
                        continue;
                    }
                    if let Some(next) = response.get("get_updates_buf").and_then(Value::as_str)
                        && !next.trim().is_empty()
                    {
                        sync_buf = next.to_string();
                    }
                    let items = response
                        .get("msgs")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    if items.is_empty() {
                        sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                    for item in items {
                        if let Err(error) = self.handle_update(item, handler.clone()).await {
                            warn!(?error, "weixin.long_poll.handle_update_failed");
                        }
                    }
                }
                Err(error) => {
                    consecutive_failures += 1;
                    warn!(?error, consecutive_failures, "weixin.long_poll.request_failed");
                    sleep(if consecutive_failures >= 3 {
                        BACKOFF_DELAY
                    } else {
                        RETRY_DELAY
                    })
                    .await;
                }
            }
        }
    }

    async fn send_event(&self, event: GatewayEvent) -> Result<()> {
        if let Some(chat_id) = typing_chat_id(&event) {
            self.start_typing_if_needed(&chat_id).await;
        }
        let messages = {
            let mut renderer = self.renderer.lock().await;
            renderer.render(event)
        };
        let final_chat_id = messages.last().map(|m| m.chat_id.clone());
        for message in messages {
            self.send_text(message).await?;
        }
        if let Some(chat_id) = final_chat_id {
            self.stop_typing_if_needed(&chat_id).await;
        }
        Ok(())
    }
}

fn typing_chat_id(event: &GatewayEvent) -> Option<String> {
    match event {
        GatewayEvent::ItemStarted { target, item, .. } => match item {
            agent_core::TranscriptItem::Reasoning { .. }
            | agent_core::TranscriptItem::CommandExecution { .. }
            | agent_core::TranscriptItem::FileChange { .. }
            | agent_core::TranscriptItem::ToolResult { .. } => Some(target.chat_id.clone()),
            _ => None,
        },
        GatewayEvent::ItemDelta { target, kind, .. } => match kind {
            crate::gateway_event::GatewayItemDeltaKind::AgentMessage
            | crate::gateway_event::GatewayItemDeltaKind::ReasoningSummary
            | crate::gateway_event::GatewayItemDeltaKind::ReasoningText => {
                Some(target.chat_id.clone())
            }
            _ => None,
        },
        GatewayEvent::ReasoningSummaryPartAdded { target, .. } => Some(target.chat_id.clone()),
        _ => None,
    }
}

#[derive(Default)]
struct SeenMessages {
    seen: HashSet<String>,
    order: VecDeque<String>,
}

impl SeenMessages {
    fn is_duplicate(&mut self, envelope: &WeixinInboundEnvelope) -> bool {
        if let Some(message_id) = envelope.message_id.as_deref()
            && self.insert_key(format!("message:{message_id}"))
        {
            return true;
        }
        let content_key = format!(
            "content:{}:{}",
            envelope.sender_user_id,
            envelope.text.trim()
        );
        self.insert_key(content_key)
    }

    fn insert_key(&mut self, key: String) -> bool {
        if key.trim().is_empty() {
            return false;
        }
        if self.seen.contains(&key) {
            return true;
        }
        self.seen.insert(key.clone());
        self.order.push_back(key);
        while self.order.len() > INBOUND_DEDUP_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        false
    }
}

fn preview(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out.replace('\n', "\\n")
}
