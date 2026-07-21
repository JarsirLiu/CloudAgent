use crate::config::GatewayConfig;
use crate::gateway_event::GatewayEvent;
use crate::message::OutboundMessage;
use crate::platform::{MessageHandler, PlatformAdapter};
use anyhow::{Context, Result};
use config::AgentConfig;
use feishu_sdk::Client;
use feishu_sdk::card::{CardAction, CardActionHandler};
use feishu_sdk::core::{Config as FeishuSdkConfig, FEISHU_BASE_URL, LARK_BASE_URL, noop_logger};
use feishu_sdk::event::{
    Event, EventDispatcher, EventDispatcherConfig, EventHandler, EventHandlerResult,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::Value;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use super::admission::{AdmissionDecision, evaluate_admission};
use super::normalize::normalize_inbound;
use super::outbound::{send_interactive_card, send_message};
use super::render::FeishuOutboundRenderer;
use super::types::{FeishuBotIdentity, FeishuMessageEnvelope};

const SEEN_EVENT_LIMIT: usize = 2048;
const BOT_MESSAGE_TRACK_LIMIT: usize = 1024;
const INBOUND_MESSAGE_DEDUP_TTL: Duration = Duration::from_secs(60);
const OLD_MESSAGE_GRACE_MS: u64 = 2_000;

static PROCESS_START_TIME_MS: OnceLock<u64> = OnceLock::new();

type CardActionCallback =
    dyn Fn(CardAction) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync;

#[derive(Clone, Default)]
pub struct FeishuAdapterOptions {
    pub enable_cards: bool,
    pub on_card_action: Option<Arc<CardActionCallback>>,
}

#[derive(Clone)]
pub struct FeishuAdapter {
    client: Client,
    gateway_config: GatewayConfig,
    options: FeishuAdapterOptions,
    seen_events: Arc<Mutex<SeenEvents>>,
    seen_events_path: PathBuf,
    recent_messages: Arc<Mutex<RecentInboundMessages>>,
    bot_messages: Arc<Mutex<TrackedBotMessages>>,
    bot_identity: Arc<Mutex<FeishuBotIdentity>>,
    renderer: Arc<Mutex<FeishuOutboundRenderer>>,
    http: reqwest::Client,
}

impl FeishuAdapter {
    pub fn new(config: GatewayConfig, options: FeishuAdapterOptions) -> Result<Self> {
        let base_url = match config.feishu.base_url.as_str() {
            "https://open.larksuite.com" => LARK_BASE_URL,
            _ => FEISHU_BASE_URL,
        };

        let sdk_config = FeishuSdkConfig::builder(&config.feishu.app_id, &config.feishu.app_secret)
            .base_url(base_url)
            .build();

        let client = Client::new(sdk_config).context("failed to build feishu client")?;
        let http = reqwest::Client::builder()
            .default_headers(default_headers()?)
            .build()
            .context("failed to build feishu http client")?;
        let seen_events_path = resolve_seen_events_path()?;
        let seen_events = load_seen_events(&seen_events_path).unwrap_or_default();

        Ok(Self {
            client,
            gateway_config: config,
            options,
            seen_events: Arc::new(Mutex::new(seen_events)),
            seen_events_path,
            recent_messages: Arc::new(Mutex::new(RecentInboundMessages::default())),
            bot_messages: Arc::new(Mutex::new(TrackedBotMessages::default())),
            bot_identity: Arc::new(Mutex::new(FeishuBotIdentity::default())),
            renderer: Arc::new(Mutex::new(FeishuOutboundRenderer::default())),
            http,
        })
    }

    async fn hydrate_bot_identity(&self) {
        info!("feishu.bot_identity.start");
        match fetch_bot_identity(&self.http, &self.gateway_config).await {
            Ok(identity) => {
                if identity.open_id.trim().is_empty() && identity.name.trim().is_empty() {
                    error!("feishu.bot_identity.empty");
                    return;
                }
                info!(
                    bot_open_id = %identity.open_id,
                    bot_name = %identity.name,
                    "feishu.bot_identity.ok"
                );
                let mut guard = self.bot_identity.lock().await;
                *guard = identity;
            }
            Err(error) => {
                error!(?error, "feishu.bot_identity.failed");
            }
        }
    }

    async fn handle_event(
        &self,
        event: Event<Value>,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<()> {
        let event_id = event.event_id().unwrap_or_default().to_string();
        let event_type = event.event_type().unwrap_or_default().to_string();
        info!(
            event_id = %event_id,
            event_type = %event_type,
            "feishu.inbound.event.received"
        );
        if !event_id.is_empty() && !self.mark_event_seen(event_id.clone()).await {
            info!(event_id = %event_id, "feishu.inbound.event.duplicate");
            return Ok(());
        }

        let payload = match event.event {
            Some(value) => value,
            None => {
                info!("feishu.inbound.event.empty_payload");
                return Ok(());
            }
        };

        let envelope: FeishuMessageEnvelope =
            serde_json::from_value(payload).context("failed to parse feishu message event")?;
        let bot_identity = self.bot_identity.lock().await.clone();
        info!(
            sender_type = ?envelope.sender.sender_type,
            sender_open_id = ?envelope.sender.sender_id.as_ref().and_then(|id| id.open_id.clone()),
            chat_id = ?envelope.message.chat_id,
            chat_type = ?envelope.message.chat_type,
            message_id = ?envelope.message.message_id,
            message_type = ?envelope.message.message_type,
            root_id = ?envelope.message.root_id,
            parent_id = ?envelope.message.parent_id,
            create_time = ?envelope.create_time,
            "feishu.inbound.event.parsed"
        );
        if let Some(message_id) = envelope.message.message_id.as_deref()
            && self.is_duplicate_message_id(message_id).await
        {
            info!(message_id = %message_id, "feishu.inbound.message.duplicate");
            return Ok(());
        }
        if let Some(create_time) = envelope.create_time.as_deref()
            && is_old_message_after_restart(create_time)
        {
            info!(create_time = %create_time, "feishu.inbound.message.old_after_restart");
            return Ok(());
        }

        let admission = evaluate_admission(
            &envelope,
            &bot_identity,
            self.gateway_config.feishu.group_only_mentioned,
            self.gateway_config.feishu.group_reply_without_mention,
            self.replied_to_known_bot_message(&envelope).await,
        );
        match admission {
            AdmissionDecision::Admitted => {
                info!("feishu.inbound.admission.accepted");
            }
            AdmissionDecision::RejectedSenderType => {
                info!("feishu.inbound.admission.rejected_sender_type");
                return Ok(());
            }
            AdmissionDecision::RejectedGroupNotMentioned => {
                info!("feishu.inbound.admission.rejected_group_not_mentioned");
                return Ok(());
            }
        }

        let Some(message) = normalize_inbound(envelope, &bot_identity) else {
            info!("feishu.inbound.normalize.none");
            return Ok(());
        };

        if message.text.is_empty() {
            info!(
                chat_id = %message.chat_id,
                message_id = %message.message_id,
                "feishu.inbound.normalize.empty_text"
            );
            return Ok(());
        }

        info!(
            chat_id = %message.chat_id,
            chat_type = ?message.chat_type,
            sender = %message.sender_open_id,
            message_id = %message.message_id,
            thread_id = ?message.thread_id,
            mentioned = message.mentioned,
            text_preview = %preview(&message.text, 120),
            reply_to_message_id = ?message.reply_context.as_ref().map(|ctx| ctx.message_id.clone()),
            "feishu.inbound.normalize.ok"
        );

        info!(
            chat_id = %message.chat_id,
            message_id = %message.message_id,
            "feishu.runtime.dispatch.start"
        );
        if handler.try_handle_session_command(&message).await? {
            info!(
                chat_id = %message.chat_id,
                message_id = %message.message_id,
                "feishu.runtime.dispatch.handled_as_session_command"
            );
            return Ok(());
        }
        handler.handle_message(message).await
    }

    async fn mark_event_seen(&self, event_id: String) -> bool {
        let mut state = self.seen_events.lock().await;
        let inserted = state.insert(event_id);
        if inserted && let Err(error) = persist_seen_events(&self.seen_events_path, &state) {
            error!(?error, path = %self.seen_events_path.display(), "feishu.seen_events.persist_failed");
        }
        inserted
    }

    async fn is_duplicate_message_id(&self, message_id: &str) -> bool {
        self.recent_messages.lock().await.insert(message_id)
    }

    pub async fn send_approval_card(
        &self,
        chat_id: String,
        reply_context: Option<crate::message::ReplyContext>,
        card: Value,
    ) -> Result<()> {
        send_interactive_card(&self.client, chat_id, reply_context, card, "approval").await
    }
}

struct FeishuMessageHandler {
    adapter: Arc<FeishuAdapter>,
    handler: Arc<dyn MessageHandler>,
}

impl EventHandler for FeishuMessageHandler {
    fn event_type(&self) -> &str {
        "im.message.receive_v1"
    }

    fn handle(
        &self,
        event: Event,
    ) -> Pin<Box<dyn Future<Output = EventHandlerResult> + Send + '_>> {
        Box::pin(async move {
            if let Err(error) = self.adapter.handle_event(event, self.handler.clone()).await {
                error!(?error, "failed to process feishu event");
            }
            Ok(None)
        })
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for FeishuAdapter {
    fn platform_name(&self) -> &'static str {
        "feishu"
    }

    async fn run(self: Arc<Self>, handler: Arc<dyn MessageHandler>) -> Result<()> {
        self.hydrate_bot_identity().await;

        let mut cfg = EventDispatcherConfig::new();
        if let Some(token) = &self.gateway_config.feishu.verification_token {
            cfg = cfg.verification_token(token.clone());
        }
        if let Some(encrypt_key) = &self.gateway_config.feishu.encrypt_key {
            cfg = cfg.encrypt_key(encrypt_key.clone());
        }

        let dispatcher = EventDispatcher::new(cfg, noop_logger());
        dispatcher
            .register_handler(Box::new(FeishuMessageHandler {
                adapter: self.clone(),
                handler,
            }))
            .await;

        info!(
            platform = self.platform_name(),
            app_id = %self.gateway_config.feishu.app_id,
            base_url = %self.gateway_config.feishu.base_url,
            group_only_mentioned = self.gateway_config.feishu.group_only_mentioned,
            group_reply_without_mention = self.gateway_config.feishu.group_reply_without_mention,
            enable_cards = self.options.enable_cards,
            "feishu.websocket.start"
        );
        let mut stream_builder = self.client.stream().event_dispatcher(dispatcher);
        if self.options.enable_cards
            && let Some(on_card_action) = self.options.on_card_action.clone()
        {
            let card_handler = CardActionHandler::new(noop_logger()).handler(move |action| {
                let on_card_action = on_card_action.clone();
                Box::pin(async move {
                    if let Err(error) = on_card_action(action).await {
                        error!(?error, "failed to process feishu card action");
                    }
                    Ok(None)
                })
            });
            stream_builder = stream_builder.card_handler(card_handler);
        }
        let stream = stream_builder
            .build()
            .context("failed to build feishu websocket stream")?;

        stream
            .start()
            .await
            .context("feishu websocket stream stopped")
    }

    async fn send_event(&self, event: GatewayEvent) -> Result<()> {
        let rendered: Vec<OutboundMessage> = {
            let mut renderer = self.renderer.lock().await;
            renderer.render(event)
        };
        if rendered.is_empty() {
            return Ok(());
        }
        for message in rendered {
            debug!(
                chat_id = %message.chat_id,
                has_reply_context = message.reply_context.is_some(),
                text_preview = %preview(&message.text, 120),
                "feishu.outbound.rendered.message"
            );
            let sent_message_ids = send_message(&self.client, message).await?;
            if !sent_message_ids.is_empty() {
                let mut tracked = self.bot_messages.lock().await;
                for message_id in sent_message_ids {
                    tracked.insert(message_id);
                }
            }
        }
        Ok(())
    }
}

impl FeishuAdapter {
    async fn replied_to_known_bot_message(&self, envelope: &FeishuMessageEnvelope) -> bool {
        let tracked = self.bot_messages.lock().await;
        envelope
            .message
            .parent_id
            .as_deref()
            .is_some_and(|id| tracked.contains(id))
            || envelope
                .message
                .root_id
                .as_deref()
                .is_some_and(|id| tracked.contains(id))
    }
}

#[derive(Default)]
struct SeenEvents {
    order: VecDeque<String>,
    set: HashSet<String>,
}

#[derive(Default)]
struct TrackedBotMessages {
    order: VecDeque<String>,
    set: HashSet<String>,
}

#[derive(Default)]
struct RecentInboundMessages {
    order: VecDeque<(String, u64)>,
    set: HashSet<String>,
}

impl TrackedBotMessages {
    fn insert(&mut self, message_id: String) {
        if self.set.contains(&message_id) {
            return;
        }
        self.set.insert(message_id.clone());
        self.order.push_back(message_id);
        while self.order.len() > BOT_MESSAGE_TRACK_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }
    }

    fn contains(&self, message_id: &str) -> bool {
        self.set.contains(message_id)
    }
}

impl RecentInboundMessages {
    fn insert(&mut self, message_id: &str) -> bool {
        if message_id.trim().is_empty() {
            return false;
        }
        let now_ms = now_ms();
        while let Some((oldest_id, seen_at_ms)) = self.order.front().cloned() {
            if now_ms.saturating_sub(seen_at_ms) <= INBOUND_MESSAGE_DEDUP_TTL.as_millis() as u64 {
                break;
            }
            self.order.pop_front();
            self.set.remove(&oldest_id);
        }
        if self.set.contains(message_id) {
            return true;
        }
        let message_id = message_id.to_string();
        self.set.insert(message_id.clone());
        self.order.push_back((message_id, now_ms));
        false
    }
}

impl SeenEvents {
    fn insert(&mut self, event_id: String) -> bool {
        if self.set.contains(&event_id) {
            return false;
        }
        self.set.insert(event_id.clone());
        self.order.push_back(event_id);
        while self.order.len() > SEEN_EVENT_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }
        true
    }
}

fn resolve_seen_events_path() -> Result<PathBuf> {
    let workspace_root =
        std::env::current_dir().context("failed to determine current directory")?;
    let agent_config = AgentConfig::load_runtime(workspace_root)?;
    let data_root = agent_config.runtime.data_root_dir;
    let platform_dir = match data_root.file_name().and_then(|name| name.to_str()) {
        Some("data") => data_root
            .parent()
            .map(|parent| parent.join("platform"))
            .unwrap_or_else(|| data_root.join("platform")),
        _ => data_root.join("platform"),
    };
    Ok(platform_dir.join("feishu.seen-events.json"))
}

fn load_seen_events(path: &PathBuf) -> Result<SeenEvents> {
    if !path.exists() {
        return Ok(SeenEvents::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read feishu seen-events {}", path.display()))?;
    let items = serde_json::from_str::<Vec<String>>(&raw)
        .with_context(|| format!("failed to parse feishu seen-events {}", path.display()))?;
    let mut state = SeenEvents::default();
    for event_id in items {
        state.insert(event_id);
    }
    Ok(state)
}

fn persist_seen_events(path: &PathBuf, state: &SeenEvents) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create feishu seen-events dir {}",
                parent.display()
            )
        })?;
    }
    let items: Vec<&String> = state.order.iter().collect();
    fs::write(path, serde_json::to_vec_pretty(&items)?)
        .with_context(|| format!("failed to write feishu seen-events {}", path.display()))?;
    Ok(())
}

fn process_start_time_ms() -> u64 {
    *PROCESS_START_TIME_MS.get_or_init(now_ms)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn is_old_message_after_restart(create_time: &str) -> bool {
    let Ok(create_time_ms) = create_time.trim().parse::<u64>() else {
        return false;
    };
    create_time_ms + OLD_MESSAGE_GRACE_MS < process_start_time_ms()
}

fn default_headers() -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(headers)
}

async fn fetch_bot_identity(
    http: &reqwest::Client,
    config: &GatewayConfig,
) -> Result<FeishuBotIdentity> {
    let token_url = format!(
        "{}/open-apis/auth/v3/tenant_access_token/internal",
        config.feishu.base_url.trim_end_matches('/')
    );
    let token_response = http
        .post(token_url)
        .json(&serde_json::json!({
            "app_id": config.feishu.app_id,
            "app_secret": config.feishu.app_secret,
        }))
        .send()
        .await
        .context("failed to request feishu tenant token")?;
    let token_json = token_response
        .json::<Value>()
        .await
        .context("failed to parse feishu tenant token response")?;
    let tenant_token = token_json
        .get("tenant_access_token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if tenant_token.is_empty() {
        anyhow::bail!("feishu tenant_access_token missing in auth response: {token_json}");
    }

    let bot_url = format!(
        "{}/open-apis/bot/v3/info",
        config.feishu.base_url.trim_end_matches('/')
    );
    let bot_response = http
        .get(bot_url)
        .header(AUTHORIZATION, format!("Bearer {tenant_token}"))
        .send()
        .await
        .context("failed to request feishu bot info")?;
    let bot_json = bot_response
        .json::<Value>()
        .await
        .context("failed to parse feishu bot info response")?;
    let identity = parse_bot_identity(&bot_json);
    if identity.open_id.trim().is_empty() && identity.name.trim().is_empty() {
        anyhow::bail!("feishu bot info missing open_id and name: {bot_json}");
    }
    Ok(identity)
}

fn parse_bot_identity(payload: &Value) -> FeishuBotIdentity {
    let bot = payload
        .get("bot")
        .or_else(|| payload.get("data").and_then(|data| data.get("bot")))
        .or_else(|| payload.get("data"))
        .cloned()
        .unwrap_or(Value::Null);

    FeishuBotIdentity {
        open_id: bot
            .get("open_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        name: bot
            .get("app_name")
            .or_else(|| bot.get("bot_name"))
            .or_else(|| bot.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
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

#[cfg(test)]
mod tests {
    use super::{RecentInboundMessages, is_old_message_after_restart, now_ms, parse_bot_identity};
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn parse_bot_identity_supports_bot_wrapper_shape() {
        let payload = json!({
            "code": 0,
            "bot": {
                "open_id": "ou_bot_123",
                "app_name": "自定义机器人"
            }
        });

        let identity = parse_bot_identity(&payload);
        assert_eq!(identity.open_id, "ou_bot_123");
        assert_eq!(identity.name, "自定义机器人");
    }

    #[test]
    fn parse_bot_identity_supports_nested_data_bot_shape() {
        let payload = json!({
            "code": 0,
            "data": {
                "bot": {
                    "open_id": "ou_bot_456",
                    "bot_name": "CloudAgent Beta"
                }
            }
        });

        let identity = parse_bot_identity(&payload);
        assert_eq!(identity.open_id, "ou_bot_456");
        assert_eq!(identity.name, "CloudAgent Beta");
    }

    #[test]
    fn recent_inbound_messages_dedups_same_message_id() {
        let mut dedup = RecentInboundMessages::default();
        assert!(!dedup.insert("om_1"));
        assert!(dedup.insert("om_1"));
        assert!(!dedup.insert("om_2"));
    }

    #[test]
    fn old_message_filter_rejects_messages_before_process_start() {
        let old_ms = now_ms().saturating_sub(Duration::from_secs(10).as_millis() as u64);
        assert!(is_old_message_after_restart(&old_ms.to_string()));
    }

    #[test]
    fn old_message_filter_keeps_fresh_messages() {
        let fresh_ms = now_ms();
        assert!(!is_old_message_after_restart(&fresh_ms.to_string()));
    }
}
