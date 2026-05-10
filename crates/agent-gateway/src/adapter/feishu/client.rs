use super::config::FeishuAdapterConfig;
use super::inbound::{FeishuChatKind, FeishuInboundMessage, FeishuReplyContext};
use super::outbound::FeishuOutboundMessage;
use anyhow::{Context, Result, anyhow};
use feishu_sdk::Client as FeishuSdkClient;
use feishu_sdk::core::{Config as FeishuSdkConfig, FEISHU_BASE_URL, LARK_BASE_URL, noop_logger};
use feishu_sdk::event::models::im::MessageEvent;
use feishu_sdk::event::{Event, EventDispatcher, EventDispatcherConfig, EventHandler, EventResp};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeishuPlatformEvent {
    Message(FeishuInboundMessage),
    CardAction {
        action: String,
        conversation_id: String,
    },
}

pub struct FeishuPlatformClient {
    config: FeishuAdapterConfig,
    http: Client,
    inbound_rx: mpsc::Receiver<FeishuPlatformEvent>,
    token_cache: Option<FeishuTokenCache>,
    reply_contexts: Arc<std::sync::Mutex<HashMap<String, FeishuReplyContext>>>,
}

impl FeishuPlatformClient {
    pub fn new(
        config: FeishuAdapterConfig,
        reply_contexts: Arc<std::sync::Mutex<HashMap<String, FeishuReplyContext>>>,
    ) -> Result<Self> {
        config.validate()?;
        let (inbound_tx, inbound_rx) = mpsc::channel(128);
        spawn_feishu_stream(config.clone(), inbound_tx.clone())?;
        Ok(Self {
            config,
            http: Client::new(),
            inbound_rx,
            token_cache: None,
            reply_contexts,
        })
    }

    pub fn config(&self) -> &FeishuAdapterConfig {
        &self.config
    }

    pub async fn next_platform_event(&mut self) -> Result<Option<FeishuPlatformEvent>> {
        Ok(self.inbound_rx.recv().await)
    }

    pub async fn send_platform_message(&mut self, message: FeishuOutboundMessage) -> Result<()> {
        let conversation_id = message.conversation_id().to_string();
        let target = FeishuSendTarget::parse(message.conversation_id()).with_context(|| {
            format!(
                "unsupported feishu conversation id: {}",
                message.conversation_id()
            )
        })?;
        let reply_context = self.lookup_reply_context(&conversation_id);
        let token = self.tenant_access_token().await?;
        if self.config.reply_to_trigger
            && let Some(reply_context) = reply_context.as_ref()
            && !reply_context.message_id.is_empty()
        {
            let request = FeishuReplyMessageRequest::from_outbound(
                message,
                self.should_reply_in_thread(reply_context),
            )?;
            self.reply_to_message(&token, reply_context, &request).await?;
            return Ok(());
        }

        let request = FeishuSendMessageRequest::from_outbound(target.receive_id.clone(), message)?;
        self.create_message(&token, &target, &request).await
    }

    async fn tenant_access_token(&mut self) -> Result<String> {
        if let Some(cache) = &self.token_cache
            && Instant::now() < cache.expires_at
        {
            return Ok(cache.token.clone());
        }

        let url = format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            self.config.domain.trim_end_matches('/')
        );
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await
            .context("request feishu tenant access token")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("read feishu tenant access token response")?;
        if !status.is_success() {
            return Err(anyhow!("feishu token request failed with {status}: {body}"));
        }
        let parsed: FeishuTenantTokenResponse =
            serde_json::from_str(&body).context("parse feishu token response")?;
        if parsed.code != 0 {
            return Err(anyhow!(
                "feishu token request rejected: code={} msg={}",
                parsed.code,
                parsed.msg
            ));
        }
        let data = parsed.into_token_data()?;
        let ttl = data.expire.saturating_sub(60) as u64;
        self.token_cache = Some(FeishuTokenCache {
            token: data.tenant_access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl.max(60)),
        });
        Ok(data.tenant_access_token)
    }

    fn lookup_reply_context(&self, conversation_id: &str) -> Option<FeishuReplyContext> {
        self.reply_contexts
            .lock()
            .ok()
            .and_then(|contexts| contexts.get(conversation_id).cloned())
    }

    fn should_reply_in_thread(&self, reply_context: &FeishuReplyContext) -> bool {
        self.config.thread_isolation && reply_context.root_id.is_some()
    }

    async fn create_message(
        &self,
        token: &str,
        target: &FeishuSendTarget,
        request: &FeishuSendMessageRequest,
    ) -> Result<()> {
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type={}",
            self.config.domain.trim_end_matches('/'),
            target.receive_id_type
        );
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(request)
            .send()
            .await
            .context("send feishu message request")?;
        parse_feishu_api_response(response, "send message").await
    }

    async fn reply_to_message(
        &self,
        token: &str,
        reply_context: &FeishuReplyContext,
        request: &FeishuReplyMessageRequest,
    ) -> Result<()> {
        let url = format!(
            "{}/open-apis/im/v1/messages/{}/reply",
            self.config.domain.trim_end_matches('/'),
            reply_context.message_id
        );
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(request)
            .send()
            .await
            .with_context(|| format!("reply to feishu message {}", reply_context.message_id))?;
        parse_feishu_api_response(response, "reply message").await
    }
}

#[derive(Clone, Debug)]
struct FeishuTokenCache {
    token: String,
    expires_at: Instant,
}

fn spawn_feishu_stream(
    config: FeishuAdapterConfig,
    inbound_tx: mpsc::Sender<FeishuPlatformEvent>,
) -> Result<()> {
    let base_url = match config.domain.trim_end_matches('/') {
        "https://open.feishu.cn" => FEISHU_BASE_URL,
        "https://open.larksuite.com" => LARK_BASE_URL,
        other => other,
    };
    let sdk_config = FeishuSdkConfig::builder(&config.app_id, &config.app_secret)
        .base_url(base_url)
        .build();
    let client = FeishuSdkClient::new(sdk_config).map_err(|err| anyhow!(err.to_string()))?;
    let dispatcher = EventDispatcher::new(EventDispatcherConfig::new(), noop_logger());
    let handler = FeishuMessageHandler {
        inbound_tx,
        thread_isolation: config.thread_isolation,
        chat_resolver: FeishuChatResolver::new(
            config.domain.clone(),
            config.app_id.clone(),
            config.app_secret.clone(),
        ),
    };

    tokio::spawn(async move {
        dispatcher.register_handler(Box::new(handler)).await;
        let stream = client
            .stream()
            .event_dispatcher_ref(Arc::new(dispatcher))
            .build();
        match stream {
            Ok(stream) => {
                info!("feishu websocket runtime started");
                if let Err(err) = stream.start().await {
                    warn!("feishu websocket runtime exited: {err}");
                }
            }
            Err(err) => warn!("failed to build feishu websocket runtime: {err}"),
        }
    });

    Ok(())
}

struct FeishuMessageHandler {
    inbound_tx: mpsc::Sender<FeishuPlatformEvent>,
    thread_isolation: bool,
    chat_resolver: FeishuChatResolver,
}

impl EventHandler for FeishuMessageHandler {
    fn event_type(&self) -> &str {
        "im.message.receive_v1"
    }

    fn handle(
        &self,
        event: Event,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EventResp>, feishu_sdk::core::Error>> + Send + '_>>
    {
        Box::pin(async move {
            info!(
                "feishu event received: event_id={:?} event_type={:?}",
                event.header.as_ref().and_then(|h| h.event_id.clone()),
                event.header.as_ref().and_then(|h| h.event_type.clone())
            );
            let payload = event.event.ok_or_else(|| {
                feishu_sdk::core::Error::InvalidEventFormat("missing event payload".to_string())
            })?;
            let payload: MessageEvent = serde_json::from_value(payload)
                .map_err(|err| feishu_sdk::core::Error::InvalidEventFormat(err.to_string()))?;
            let Some(sender_id) = payload.sender.sender_id.and_then(|id| id.open_id) else {
                return Ok(None);
            };
            let Some(message) = payload.message.message_type.clone() else {
                return Ok(None);
            };
            let Some(chat_id) = payload.message.chat_id.clone() else {
                return Ok(None);
            };
            let Some(content) = payload.message.content.clone() else {
                return Ok(None);
            };
            let chat_kind = self
                .chat_resolver
                .resolve_chat_kind(&chat_id)
                .await
                .map_err(|err| feishu_sdk::core::Error::InvalidEventFormat(err.to_string()))?;
            let root_id = if self.thread_isolation {
                payload.message.root_id.clone()
            } else {
                None
            };

            if let Some(inbound) =
                FeishuInboundMessage::from_event(
                    sender_id,
                    chat_id,
                    chat_kind,
                    message,
                    content,
                    payload.message.message_id.unwrap_or_default(),
                    root_id,
                )
                    .map_err(|err| feishu_sdk::core::Error::InvalidEventFormat(err.to_string()))?
            {
                info!("feishu inbound message normalized: {:?}", inbound);
                let _ = self
                    .inbound_tx
                    .send(FeishuPlatformEvent::Message(inbound))
                    .await;
            }
            Ok(None)
        })
    }
}

#[derive(Clone, Debug)]
struct FeishuSendTarget {
    receive_id_type: &'static str,
    receive_id: String,
}

#[derive(Clone)]
struct FeishuChatResolver {
    domain: String,
    app_id: String,
    app_secret: String,
    http: Client,
    token_cache: Arc<Mutex<Option<FeishuTokenCache>>>,
    chat_kind_cache: Arc<Mutex<HashMap<String, FeishuChatKind>>>,
}

impl FeishuChatResolver {
    fn new(domain: String, app_id: String, app_secret: String) -> Self {
        Self {
            domain,
            app_id,
            app_secret,
            http: Client::new(),
            token_cache: Arc::new(Mutex::new(None)),
            chat_kind_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn resolve_chat_kind(&self, chat_id: &str) -> Result<FeishuChatKind> {
        if let Some(kind) = self.chat_kind_cache.lock().await.get(chat_id).copied() {
            return Ok(kind);
        }

        let token = self.tenant_access_token().await?;
        let url = format!(
            "{}/open-apis/im/v1/chats/{}",
            self.domain.trim_end_matches('/'),
            chat_id
        );
        let response = self
            .http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .context("request feishu chat info")?;
        let status = response.status();
        let body = response.text().await.context("read feishu chat info response")?;
        if !status.is_success() {
            return Err(anyhow!("feishu chat info failed with {status}: {body}"));
        }
        let parsed: FeishuApiResponse<FeishuChatInfoData> =
            serde_json::from_str(&body).context("parse feishu chat info response")?;
        if parsed.code != 0 {
            return Err(anyhow!(
                "feishu chat info rejected: code={} msg={}",
                parsed.code,
                parsed.msg
            ));
        }
        let kind = match parsed
            .data
            .and_then(|data| data.chat)
            .and_then(|chat| chat.chat_type)
            .as_deref()
        {
            Some("p2p") => FeishuChatKind::P2p,
            _ => FeishuChatKind::Group,
        };
        self.chat_kind_cache
            .lock()
            .await
            .insert(chat_id.to_string(), kind);
        Ok(kind)
    }

    async fn tenant_access_token(&self) -> Result<String> {
        if let Some(cache) = self.token_cache.lock().await.clone()
            && Instant::now() < cache.expires_at
        {
            return Ok(cache.token);
        }

        let url = format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            self.domain.trim_end_matches('/')
        );
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .context("request feishu tenant access token for chat resolver")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("read feishu tenant access token response for chat resolver")?;
        if !status.is_success() {
            return Err(anyhow!("feishu token request failed with {status}: {body}"));
        }
        let parsed: FeishuTenantTokenResponse =
            serde_json::from_str(&body).context("parse feishu token response for chat resolver")?;
        if parsed.code != 0 {
            return Err(anyhow!(
                "feishu token request rejected: code={} msg={}",
                parsed.code,
                parsed.msg
            ));
        }
        let data = parsed.into_token_data()?;
        let ttl = data.expire.saturating_sub(60) as u64;
        let cache = FeishuTokenCache {
            token: data.tenant_access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl.max(60)),
        };
        *self.token_cache.lock().await = Some(cache);
        Ok(data.tenant_access_token)
    }
}

impl FeishuSendTarget {
    fn parse(conversation_id: &str) -> Result<Self> {
        if let Some(open_id) = conversation_id.strip_prefix("feishu:p2p:") {
            return Ok(Self {
                receive_id_type: "open_id",
                receive_id: open_id.to_string(),
            });
        }
        if let Some(chat_id) = conversation_id.strip_prefix("feishu:chat:") {
            let chat_id = chat_id.split(":thread:").next().unwrap_or(chat_id);
            return Ok(Self {
                receive_id_type: "chat_id",
                receive_id: chat_id.to_string(),
            });
        }
        Err(anyhow!(
            "feishu conversation id must start with feishu:p2p: or feishu:chat:"
        ))
    }
}

#[derive(Serialize)]
struct FeishuSendMessageRequest {
    receive_id: String,
    msg_type: &'static str,
    content: String,
}

#[derive(Serialize)]
struct FeishuReplyMessageRequest {
    content: String,
    msg_type: &'static str,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    reply_in_thread: bool,
}

impl FeishuSendMessageRequest {
    fn from_outbound(receive_id: String, message: FeishuOutboundMessage) -> Result<Self> {
        match message {
            FeishuOutboundMessage::Text { text, .. } => Ok(Self {
                receive_id,
                msg_type: "text",
                content: serde_json::to_string(&serde_json::json!({ "text": text }))?,
            }),
            FeishuOutboundMessage::Progress { summary, .. } => Ok(Self {
                receive_id,
                msg_type: "text",
                content: serde_json::to_string(&serde_json::json!({ "text": summary }))?,
            }),
            FeishuOutboundMessage::Card { body, .. }
            | FeishuOutboundMessage::ApprovalCard { body, .. } => Ok(Self {
                receive_id,
                msg_type: "interactive",
                content: body,
            }),
        }
    }
}

impl FeishuReplyMessageRequest {
    fn from_outbound(message: FeishuOutboundMessage, reply_in_thread: bool) -> Result<Self> {
        match message {
            FeishuOutboundMessage::Text { text, .. } => Ok(Self {
                msg_type: "text",
                content: serde_json::to_string(&serde_json::json!({ "text": text }))?,
                reply_in_thread,
            }),
            FeishuOutboundMessage::Progress { summary, .. } => Ok(Self {
                msg_type: "text",
                content: serde_json::to_string(&serde_json::json!({ "text": summary }))?,
                reply_in_thread,
            }),
            FeishuOutboundMessage::Card { body, .. }
            | FeishuOutboundMessage::ApprovalCard { body, .. } => Ok(Self {
                msg_type: "interactive",
                content: body,
                reply_in_thread,
            }),
        }
    }
}

async fn parse_feishu_api_response(response: reqwest::Response, operation: &str) -> Result<()> {
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("read feishu {operation} response"))?;
    if !status.is_success() {
        return Err(anyhow!("feishu {operation} failed with {status}: {body}"));
    }
    let parsed: FeishuApiResponse<serde_json::Value> =
        serde_json::from_str(&body).with_context(|| format!("parse feishu {operation} response"))?;
    if parsed.code != 0 {
        return Err(anyhow!(
            "feishu {operation} rejected: code={} msg={}",
            parsed.code,
            parsed.msg
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct FeishuApiResponse<T> {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<T>,
}

#[derive(Default, Deserialize)]
struct FeishuTenantTokenData {
    tenant_access_token: String,
    expire: i64,
}

#[derive(Default, Deserialize)]
struct FeishuTenantTokenResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<FeishuTenantTokenData>,
    #[serde(default)]
    tenant_access_token: String,
    #[serde(default)]
    expire: i64,
}

impl FeishuTenantTokenResponse {
    fn into_token_data(self) -> Result<FeishuTenantTokenData> {
        if let Some(data) = self.data {
            return Ok(data);
        }
        if !self.tenant_access_token.is_empty() {
            return Ok(FeishuTenantTokenData {
                tenant_access_token: self.tenant_access_token,
                expire: self.expire,
            });
        }
        Err(anyhow!("feishu token response missing data"))
    }
}

#[derive(Default, Deserialize)]
struct FeishuChatInfoData {
    #[serde(default)]
    chat: Option<FeishuChatInfo>,
}

#[derive(Default, Deserialize)]
struct FeishuChatInfo {
    #[serde(default)]
    chat_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::FeishuReplyMessageRequest;
    use crate::adapter::feishu::inbound::{FeishuChatKind, FeishuInboundMessage};
    use crate::adapter::feishu::FeishuPlatformEvent;
    use crate::adapter::feishu::outbound::FeishuOutboundMessage;
    use serde_json::Value;

    #[test]
    fn feishu_event_text_message_maps_to_platform_event() {
        let message = FeishuInboundMessage::from_event(
            "ou_user".to_string(),
            "oc_chat".to_string(),
            FeishuChatKind::Group,
            "text".to_string(),
            "{\"text\":\"hello\"}".to_string(),
            "om_123".to_string(),
            None,
        )
        .expect("parse")
        .expect("some");

        match FeishuPlatformEvent::Message(message) {
            FeishuPlatformEvent::Message(message) => {
                let gateway = message.into_gateway_message();
                assert_eq!(gateway.conversation_id, "feishu:chat:oc_chat");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn thread_event_captures_root_reply_context() {
        let message = FeishuInboundMessage::from_event(
            "ou_user".to_string(),
            "oc_chat".to_string(),
            FeishuChatKind::Group,
            "text".to_string(),
            "{\"text\":\"hello\"}".to_string(),
            "om_child".to_string(),
            Some("om_root".to_string()),
        )
        .expect("parse")
        .expect("some");

        assert_eq!(message.conversation_id(), "feishu:chat:oc_chat:thread:om_root");
        let reply_context = message.reply_context();
        assert_eq!(reply_context.chat_id, "oc_chat");
        assert_eq!(reply_context.message_id, "om_child");
        assert_eq!(reply_context.root_id.as_deref(), Some("om_root"));
    }

    #[test]
    fn reply_request_marks_thread_reply_when_enabled() {
        let request = FeishuReplyMessageRequest::from_outbound(
            FeishuOutboundMessage::Text {
                conversation_id: "feishu:chat:oc_chat:thread:om_root".to_string(),
                text: "hello".to_string(),
            },
            true,
        )
        .expect("request");

        let value: Value = serde_json::to_value(request).expect("serialize");
        assert_eq!(value["msg_type"], "text");
        assert_eq!(value["reply_in_thread"], true);
    }
}
