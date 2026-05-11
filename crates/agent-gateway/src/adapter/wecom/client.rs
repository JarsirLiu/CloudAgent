use super::config::{WecomAdapterConfig, WecomPolicy};
use super::inbound::WecomInboundEnvelope;
use super::outbound::{WecomOutboundMessage, WecomOutboundRenderer};
use crate::gateway_outbound::GatewayOutbound;
use crate::message::InboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use crate::session::build_session_key;
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{debug, info, warn};

const DEFAULT_WEBSOCKET_URL: &str = "wss://openws.work.weixin.qq.com";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const INBOUND_DEDUP_LIMIT: usize = 1024;
const SEND_ACK_TIMEOUT: Duration = Duration::from_secs(15);
const TEXT_BATCH_DELAY: Duration = Duration::from_millis(600);
const TEXT_BATCH_SPLIT_DELAY: Duration = Duration::from_secs(2);
const TEXT_SPLIT_THRESHOLD: usize = 3900;
const RECONNECT_BACKOFF: [Duration; 5] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
];
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWriter = futures_util::stream::SplitSink<WsStream, Message>;
type WsReader = futures_util::stream::SplitStream<WsStream>;

#[derive(Clone)]
pub struct WecomAdapter {
    config: WecomAdapterConfig,
    websocket_url: String,
    writer: Arc<Mutex<Option<WsWriter>>>,
    heartbeat_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    http: reqwest::Client,
    renderer: Arc<Mutex<WecomOutboundRenderer>>,
    reply_req_ids: Arc<Mutex<HashMap<String, String>>>,
    latest_chat_req_ids: Arc<Mutex<HashMap<String, String>>>,
    seen_messages: Arc<Mutex<SeenMessages>>,
    pending_acks: Arc<Mutex<HashMap<String, oneshot::Sender<Result<()>>>>>,
    pending_text_batches: Arc<Mutex<HashMap<String, PendingTextBatch>>>,
    pending_text_batch_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl WecomAdapter {
    pub fn new(config: WecomAdapterConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            websocket_url: DEFAULT_WEBSOCKET_URL.to_string(),
            writer: Arc::new(Mutex::new(None)),
            heartbeat_task: Arc::new(Mutex::new(None)),
            http: reqwest::Client::new(),
            renderer: Arc::new(Mutex::new(WecomOutboundRenderer::default())),
            reply_req_ids: Arc::new(Mutex::new(HashMap::new())),
            latest_chat_req_ids: Arc::new(Mutex::new(HashMap::new())),
            seen_messages: Arc::new(Mutex::new(SeenMessages::default())),
            pending_acks: Arc::new(Mutex::new(HashMap::new())),
            pending_text_batches: Arc::new(Mutex::new(HashMap::new())),
            pending_text_batch_tasks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn connect(&self) -> Result<WsReader> {
        let (mut stream, response) = connect_async(self.websocket_url.as_str())
            .await
            .with_context(|| format!("failed to connect {}", self.websocket_url))?;
        debug!(
            status = ?response.status(),
            "wecom.websocket.connected"
        );

        let subscribe_req_id = new_req_id("subscribe");
        let subscribe_frame = json!({
            "cmd": "aibot_subscribe",
            "headers": { "req_id": subscribe_req_id },
            "body": {
                "bot_id": self.config.bot_id,
                "secret": self.config.bot_secret,
                "device_id": format!("cloudagent-{}", std::process::id()),
            }
        });
        stream
            .send(Message::Text(subscribe_frame.to_string().into()))
            .await
            .context("failed to send wecom subscribe frame")?;

        loop {
            let message = stream
                .next()
                .await
                .transpose()
                .context("failed to read wecom subscribe response")?
                .context("wecom websocket closed during subscribe")?;
            let Message::Text(text) = message else {
                continue;
            };
            let payload: Value = serde_json::from_str(&text)
                .context("failed to decode wecom subscribe response")?;
            let req_id = payload
                .get("headers")
                .and_then(|headers| headers.get("req_id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if req_id != subscribe_req_id {
                continue;
            }
            let errcode = payload.get("errcode").and_then(Value::as_i64).unwrap_or(0);
            if errcode != 0 {
                anyhow::bail!("wecom subscribe failed: {payload}");
            }
            break;
        }

        let (writer, reader) = stream.split();
        *self.writer.lock().await = Some(writer);
        self.start_heartbeat().await;
        Ok(reader)
    }

    async fn start_heartbeat(&self) {
        self.stop_heartbeat().await;
        let writer = self.writer.clone();
        let task = tokio::spawn(async move {
            loop {
                sleep(HEARTBEAT_INTERVAL).await;
                let payload = json!({
                    "cmd": "ping",
                    "headers": { "req_id": new_req_id("ping") },
                    "body": {}
                });
                let mut guard = writer.lock().await;
                let Some(writer) = guard.as_mut() else {
                    break;
                };
                if writer
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        *self.heartbeat_task.lock().await = Some(task);
    }

    async fn stop_heartbeat(&self) {
        if let Some(task) = self.heartbeat_task.lock().await.take() {
            task.abort();
        }
    }

    async fn clear_connection(&self) {
        self.stop_heartbeat().await;
        *self.writer.lock().await = None;
        let mut pending_acks = self.pending_acks.lock().await;
        for (_, sender) in pending_acks.drain() {
            let _ = sender.send(Err(anyhow::anyhow!("wecom websocket disconnected")));
        }
    }

    async fn enqueue_or_dispatch(
        &self,
        message: InboundMessage,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<()> {
        if !should_batch_text(&message) {
            return self.dispatch_message(message, handler).await;
        }

        let key = build_session_key(&message);
        let chunk_len = message.text.chars().count();
        {
            let mut batches = self.pending_text_batches.lock().await;
            match batches.get_mut(&key) {
                Some(existing) => {
                    if !message.text.is_empty() {
                        if !existing.message.text.is_empty() {
                            existing.message.text.push('\n');
                        }
                        existing.message.text.push_str(&message.text);
                    }
                    existing.last_chunk_len = chunk_len;
                    if existing.message.reply_context.is_none() {
                        existing.message.reply_context = message.reply_context.clone();
                    }
                    existing.message.mentioned |= message.mentioned;
                }
                None => {
                    batches.insert(
                        key.clone(),
                        PendingTextBatch {
                            message,
                            last_chunk_len: chunk_len,
                        },
                    );
                }
            }
        }

        if let Some(task) = self.pending_text_batch_tasks.lock().await.remove(&key) {
            task.abort();
        }
        let adapter = self.clone();
        let handler_for_task = handler.clone();
        let task_key = key.clone();
        let task = tokio::spawn(async move {
            let delay = {
                let batches = adapter.pending_text_batches.lock().await;
                let last_chunk_len = batches
                    .get(&task_key)
                    .map(|batch| batch.last_chunk_len)
                    .unwrap_or_default();
                if last_chunk_len >= TEXT_SPLIT_THRESHOLD {
                    TEXT_BATCH_SPLIT_DELAY
                } else {
                    TEXT_BATCH_DELAY
                }
            };
            sleep(delay).await;
            let batch = adapter.pending_text_batches.lock().await.remove(&task_key);
            adapter.pending_text_batch_tasks.lock().await.remove(&task_key);
            let Some(batch) = batch else {
                return;
            };
            if let Err(error) = adapter.dispatch_message(batch.message, handler_for_task).await {
                warn!(?error, session_key = %task_key, "wecom.websocket.flush_batch_failed");
            }
        });
        self.pending_text_batch_tasks.lock().await.insert(key, task);
        Ok(())
    }

    async fn dispatch_message(
        &self,
        inbound: InboundMessage,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<()> {
        if !self.is_message_allowed(&inbound) {
            debug!(
                chat_id = %inbound.chat_id,
                message_id = %inbound.message_id,
                "wecom.websocket.message_blocked_by_policy"
            );
            return Ok(());
        }
        if handler.try_handle_session_command(&inbound).await? {
            return Ok(());
        }
        handler.handle_message(inbound).await
    }

    fn is_message_allowed(&self, message: &InboundMessage) -> bool {
        match message.chat_type.as_deref() {
            Some("group") => self.is_group_allowed(message),
            _ => self.is_dm_allowed(message),
        }
    }

    fn is_dm_allowed(&self, message: &InboundMessage) -> bool {
        let sender_id = message
            .sender_user_id
            .as_deref()
            .unwrap_or(message.sender_open_id.as_str());
        match self.config.dm_policy {
            WecomPolicy::Open => true,
            WecomPolicy::Disabled => false,
            WecomPolicy::Allowlist => matches_entry(&self.config.allow_from, sender_id),
        }
    }

    fn is_group_allowed(&self, message: &InboundMessage) -> bool {
        if !message.mentioned && message.reply_context.is_none() {
            return false;
        }
        match self.config.group_policy {
            WecomPolicy::Open => true,
            WecomPolicy::Disabled => false,
            WecomPolicy::Allowlist => {
                matches_entry(&self.config.group_allow_from, &message.chat_id)
            }
        }
    }

    async fn handle_ws_payload(
        &self,
        payload: Value,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<()> {
        let cmd = payload
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        match cmd.as_str() {
            "aibot_msg_callback" | "aibot_callback" => {
                let Some(envelope) = WecomInboundEnvelope::from_payload(&payload) else {
                    return Ok(());
                };
                if self.seen_messages.lock().await.insert(&envelope.message_id) {
                    return Ok(());
                }
                if let Some(req_id) = envelope.reply_req_id.clone() {
                    self.reply_req_ids
                        .lock()
                        .await
                        .insert(envelope.message_id.clone(), req_id.clone());
                    self.latest_chat_req_ids
                        .lock()
                        .await
                        .insert(envelope.chat_id.clone(), req_id);
                }
                let envelope = strip_group_leading_mention(envelope, &self.config.bot_id);
                let image_paths = self.cache_images(&envelope.image_urls).await;
                let inbound = envelope.into_gateway_message(image_paths);
                if inbound.text.is_empty() && inbound.image_paths.is_empty() {
                    return Ok(());
                }
                self.enqueue_or_dispatch(inbound, handler).await?;
            }
            "ping" | "aibot_event_callback" => {}
            "" => {
                if let Some(req_id) = payload
                    .get("headers")
                    .and_then(|headers| headers.get("req_id"))
                    .and_then(Value::as_str)
                {
                    if req_id.starts_with("ping-") {
                        return Ok(());
                    }
                    let result = if payload.get("errcode").and_then(Value::as_i64).unwrap_or(0) == 0
                    {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!("wecom ack error: {payload}"))
                    };
                    if let Some(sender) = self.pending_acks.lock().await.remove(req_id) {
                        let _ = sender.send(result);
                    }
                }
            }
            other => {
                debug!(cmd = %other, "wecom.websocket.ignored");
            }
        }
        Ok(())
    }

    async fn send_markdown(
        &self,
        message: WecomOutboundMessage,
    ) -> Result<()> {
        let reply_req_id = {
            let reply_req_ids = self.reply_req_ids.lock().await;
            message
                .reply_context
                .as_ref()
                .and_then(|ctx| reply_req_ids.get(&ctx.message_id).cloned())
        };
        let proactive_req_id = if reply_req_id.is_none() {
            let latest_chat_req_ids = self.latest_chat_req_ids.lock().await;
            latest_chat_req_ids.get(&message.chat_id).cloned()
        } else {
            None
        };

        let payload = if let Some(reply_req_id) = reply_req_id.or(proactive_req_id) {
            json!({
                "cmd": "aibot_respond_msg",
                "headers": { "req_id": reply_req_id },
                "body": {
                    "msgtype": "markdown",
                    "markdown": { "content": message.content }
                }
            })
        } else {
            json!({
                "cmd": "aibot_send_msg",
                "headers": { "req_id": new_req_id("send") },
                "body": {
                    "chatid": message.chat_id,
                    "msgtype": "markdown",
                    "markdown": { "content": message.content }
                }
            })
        };

        let mut guard = self.writer.lock().await;
        let Some(writer) = guard.as_mut() else {
            anyhow::bail!("wecom websocket is not connected")
        };
        let req_id = payload
            .get("headers")
            .and_then(|headers| headers.get("req_id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("missing req_id in outbound payload")?;
        let (ack_tx, ack_rx) = oneshot::channel();
        self.pending_acks.lock().await.insert(req_id.clone(), ack_tx);
        writer
            .send(Message::Text(payload.to_string().into()))
            .await
            .context("failed to send wecom outbound message")?;
        drop(guard);
        match tokio::time::timeout(SEND_ACK_TIMEOUT, ack_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(anyhow::anyhow!("wecom ack channel closed for {req_id}")),
            Err(_) => {
                self.pending_acks.lock().await.remove(&req_id);
                Ok(())
            }
        }
    }

    async fn cache_images(&self, urls: &[String]) -> Vec<String> {
        let mut paths = Vec::new();
        for (index, url) in urls.iter().enumerate() {
            match self.download_image_to_temp(url, index).await {
                Ok(path) => paths.push(path),
                Err(error) => warn!(?error, image_url = %url, "wecom.websocket.cache_image_failed"),
            }
        }
        paths
    }

    async fn download_image_to_temp(&self, url: &str, index: usize) -> Result<String> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("failed to download image {url}"))?;
        let response = response
            .error_for_status()
            .with_context(|| format!("image download returned error status: {url}"))?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("image/jpeg")
            .to_string();
        let bytes = response.bytes().await.context("failed to read image bytes")?;
        let ext = guess_image_extension(&content_type, url);
        let file_name = format!("cloudagent-wecom-{}-{}{}", new_req_id("img"), index, ext);
        let path = std::env::temp_dir().join(file_name);
        fs::write(&path, &bytes).with_context(|| format!("failed to write image cache {:?}", path))?;
        Ok(path.display().to_string())
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for WecomAdapter {
    fn platform_name(&self) -> &'static str {
        "wecom"
    }

    async fn run(self: Arc<Self>, handler: Arc<dyn MessageHandler>) -> Result<()> {
        let mut backoff_index = 0usize;
        loop {
            let mut reader = match self.connect().await {
                Ok(reader) => {
                    info!(
                        websocket_url = %self.websocket_url,
                        bot_id = %self.config.bot_id,
                        "wecom.websocket.start"
                    );
                    backoff_index = 0;
                    reader
                }
                Err(error) => {
                    let delay = RECONNECT_BACKOFF[backoff_index.min(RECONNECT_BACKOFF.len() - 1)];
                    backoff_index = backoff_index.saturating_add(1);
                    warn!(?error, delay_secs = delay.as_secs(), "wecom.websocket.connect_failed");
                    sleep(delay).await;
                    continue;
                }
            };

            loop {
                match reader.next().await {
                    Some(Ok(Message::Text(text))) => {
                        let payload: Value = match serde_json::from_str(&text) {
                            Ok(payload) => payload,
                            Err(error) => {
                                warn!(?error, raw = %text, "wecom.websocket.invalid_json");
                                continue;
                            }
                        };
                        if let Err(error) = self.handle_ws_payload(payload, handler.clone()).await {
                            warn!(?error, "wecom.websocket.handle_payload_failed");
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        info!(?frame, "wecom.websocket.closed");
                        break;
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        warn!(?error, "wecom.websocket.read_failed");
                        break;
                    }
                    None => break,
                }
            }

            self.clear_connection().await;
            let delay = RECONNECT_BACKOFF[backoff_index.min(RECONNECT_BACKOFF.len() - 1)];
            backoff_index = backoff_index.saturating_add(1);
            sleep(delay).await;
        }
    }

    async fn send_outbound(&self, outbound: GatewayOutbound) -> Result<()> {
        let messages = {
            let mut renderer = self.renderer.lock().await;
            renderer.render(outbound)
        };
        for message in messages {
            self.send_markdown(message).await?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct SeenMessages {
    seen: HashSet<String>,
    order: VecDeque<String>,
}

struct PendingTextBatch {
    message: InboundMessage,
    last_chunk_len: usize,
}

impl SeenMessages {
    fn insert(&mut self, message_id: &str) -> bool {
        if message_id.trim().is_empty() {
            return false;
        }
        if self.seen.contains(message_id) {
            return true;
        }
        self.seen.insert(message_id.to_string());
        self.order.push_back(message_id.to_string());
        while self.order.len() > INBOUND_DEDUP_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        false
    }
}

fn new_req_id(prefix: &str) -> String {
    let seq = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{seq}")
}

fn should_batch_text(message: &InboundMessage) -> bool {
    !message.text.is_empty()
        && message.image_paths.is_empty()
        && message.thread_id.is_none()
        && message.reply_context.is_none()
        && !message.text.trim_start().starts_with('/')
}

fn matches_entry(entries: &[String], target: &str) -> bool {
    let normalized_target = target.trim().to_ascii_lowercase();
    entries.iter().any(|entry| {
        let normalized = entry
            .trim()
            .trim_start_matches("wecom:")
            .trim_start_matches("user:")
            .trim_start_matches("group:")
            .to_ascii_lowercase();
        normalized == "*" || normalized == normalized_target
    })
}

fn strip_group_leading_mention(
    mut envelope: WecomInboundEnvelope,
    bot_id: &str,
) -> WecomInboundEnvelope {
    if envelope.chat_kind != super::inbound::WecomChatKind::Group {
        return envelope;
    }
    let original = envelope.text.trim_start().to_string();
    let stripped = strip_leading_mention_token(&original, bot_id);
    if stripped != original {
        envelope.mentioned = true;
        envelope.text = stripped;
    } else if original.starts_with('@') {
        envelope.mentioned = true;
    }
    envelope
}

fn strip_leading_mention_token(text: &str, bot_id: &str) -> String {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('@') {
        return trimmed.to_string();
    }
    let remainder = trimmed.trim_start_matches('@');
    let split_at = remainder
        .find(char::is_whitespace)
        .unwrap_or(remainder.len());
    let mention = &remainder[..split_at];
    let rest = remainder[split_at..].trim_start();
    if mention.is_empty() {
        return trimmed.to_string();
    }
    if !bot_id.trim().is_empty() && mention.eq_ignore_ascii_case(bot_id.trim()) {
        return rest.to_string();
    }
    if mention.len() <= 64 {
        return rest.to_string();
    }
    trimmed.to_string()
}

fn guess_image_extension(content_type: &str, url: &str) -> &'static str {
    let lower = content_type.to_ascii_lowercase();
    if lower.contains("png") || url.to_ascii_lowercase().contains(".png") {
        ".png"
    } else if lower.contains("gif") || url.to_ascii_lowercase().contains(".gif") {
        ".gif"
    } else if lower.contains("webp") || url.to_ascii_lowercase().contains(".webp") {
        ".webp"
    } else {
        ".jpg"
    }
}

#[cfg(test)]
mod tests {
    use super::{matches_entry, strip_group_leading_mention};
    use crate::adapter::wecom::config::{WecomAdapterConfig, WecomPolicy};
    use crate::adapter::wecom::runtime::build_turn_content_for_tests;
    use crate::message::ReplyContext;
    use super::super::inbound::{WecomChatKind, WecomInboundEnvelope};
    use agent_core::{AttachmentRef, InputItem};

    #[test]
    fn strips_group_leading_mention() {
        let envelope = WecomInboundEnvelope {
            chat_kind: WecomChatKind::Group,
            chat_id: "group1".to_string(),
            sender_user_id: "user1".to_string(),
            message_id: "msg1".to_string(),
            text: "@cloudagent /approve".to_string(),
            image_urls: Vec::new(),
            mentioned: false,
            reply_to_message_id: None,
            reply_req_id: Some("req1".to_string()),
        };
        let normalized = strip_group_leading_mention(envelope, "cloudagent");
        assert_eq!(normalized.text, "/approve");
        assert!(normalized.mentioned);
    }

    #[test]
    fn matches_entry_supports_wildcards_and_prefixes() {
        assert!(matches_entry(&["*".to_string()], "user1"));
        assert!(matches_entry(&["wecom:user:user1".to_string()], "user1"));
        assert!(matches_entry(&["group:chat1".to_string()], "chat1"));
        assert!(!matches_entry(&["user2".to_string()], "user1"));
    }

    #[test]
    fn skips_group_message_without_mention_or_reply() {
        let message = crate::message::InboundMessage {
            platform: "wecom".to_string(),
            tenant_key: None,
            chat_id: "group1".to_string(),
            chat_type: Some("group".to_string()),
            sender_open_id: "user1".to_string(),
            sender_user_id: Some("user1".to_string()),
            sender_union_id: None,
            message_id: "msg1".to_string(),
            thread_id: None,
            text: "hello".to_string(),
            image_paths: Vec::new(),
            mentioned: false,
            reply_context: None,
        };
        let config = WecomAdapterConfig {
            bot_id: "bot".to_string(),
            bot_secret: "secret".to_string(),
            ..Default::default()
        };
        let adapter = super::WecomAdapter::new(config).expect("adapter");
        assert!(!adapter.is_group_allowed(&message));

        let replied = crate::message::InboundMessage {
            reply_context: Some(ReplyContext {
                message_id: "msg0".to_string(),
                thread_id: None,
            }),
            ..message
        };
        assert!(adapter.is_group_allowed(&replied));
    }

    #[test]
    fn allowlist_policies_are_enforced() {
        let config = WecomAdapterConfig {
            bot_id: "bot".to_string(),
            bot_secret: "secret".to_string(),
            dm_policy: WecomPolicy::Allowlist,
            group_policy: WecomPolicy::Allowlist,
            allow_from: vec!["user1".to_string()],
            group_allow_from: vec!["chat1".to_string()],
        };
        let adapter = super::WecomAdapter::new(config).expect("adapter");
        let dm = crate::message::InboundMessage {
            platform: "wecom".to_string(),
            tenant_key: None,
            chat_id: "user1".to_string(),
            chat_type: Some("p2p".to_string()),
            sender_open_id: "user1".to_string(),
            sender_user_id: Some("user1".to_string()),
            sender_union_id: None,
            message_id: "msg1".to_string(),
            thread_id: None,
            text: "hello".to_string(),
            image_paths: Vec::new(),
            mentioned: true,
            reply_context: None,
        };
        assert!(adapter.is_dm_allowed(&dm));

        let blocked_dm = crate::message::InboundMessage {
            sender_open_id: "user2".to_string(),
            sender_user_id: Some("user2".to_string()),
            chat_id: "user2".to_string(),
            ..dm.clone()
        };
        assert!(!adapter.is_dm_allowed(&blocked_dm));

        let group = crate::message::InboundMessage {
            chat_id: "chat1".to_string(),
            chat_type: Some("group".to_string()),
            ..dm
        };
        assert!(adapter.is_group_allowed(&group));

        let blocked_group = crate::message::InboundMessage {
            chat_id: "chat2".to_string(),
            ..group
        };
        assert!(!adapter.is_group_allowed(&blocked_group));
    }

    #[test]
    fn multiple_images_are_forwarded_as_local_path_inputs() {
        let message = crate::message::InboundMessage {
            platform: "wecom".to_string(),
            tenant_key: None,
            chat_id: "chat1".to_string(),
            chat_type: Some("p2p".to_string()),
            sender_open_id: "user1".to_string(),
            sender_user_id: Some("user1".to_string()),
            sender_union_id: None,
            message_id: "msg1".to_string(),
            thread_id: None,
            text: "please inspect".to_string(),
            image_paths: vec![
                "D:\\temp\\image1.png".to_string(),
                "D:\\temp\\image2.jpg".to_string(),
            ],
            mentioned: true,
            reply_context: None,
        };

        let content = build_turn_content_for_tests(&message);
        assert_eq!(content.len(), 3);
        assert_eq!(
            content[0],
            InputItem::Text {
                text: "please inspect".to_string()
            }
        );
        match &content[1] {
            InputItem::Image {
                source: AttachmentRef::LocalPath { path },
                ..
            } => assert_eq!(path, "D:\\temp\\image1.png"),
            other => panic!("expected first image local path, got {other:?}"),
        }
        match &content[2] {
            InputItem::Image {
                source: AttachmentRef::LocalPath { path },
                ..
            } => assert_eq!(path, "D:\\temp\\image2.jpg"),
            other => panic!("expected second image local path, got {other:?}"),
        }
    }
}
