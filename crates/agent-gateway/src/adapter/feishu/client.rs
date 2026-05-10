use super::config::FeishuAdapterConfig;
use super::inbound::FeishuInboundMessage;
use super::outbound::FeishuOutboundMessage;
use anyhow::{Context, Result, anyhow};
use feishu_sdk::Client as FeishuSdkClient;
use feishu_sdk::core::{Config as FeishuSdkConfig, FEISHU_BASE_URL, LARK_BASE_URL, noop_logger};
use feishu_sdk::event::models::im::MessageEvent;
use feishu_sdk::event::{Event, EventDispatcher, EventDispatcherConfig, EventHandler, EventResp};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
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
}

impl FeishuPlatformClient {
    pub fn new(config: FeishuAdapterConfig) -> Result<Self> {
        config.validate()?;
        let (inbound_tx, inbound_rx) = mpsc::channel(128);
        spawn_feishu_stream(config.clone(), inbound_tx.clone())?;
        Ok(Self {
            config,
            http: Client::new(),
            inbound_rx,
            token_cache: None,
        })
    }

    pub fn config(&self) -> &FeishuAdapterConfig {
        &self.config
    }

    pub async fn next_platform_event(&mut self) -> Result<Option<FeishuPlatformEvent>> {
        Ok(self.inbound_rx.recv().await)
    }

    pub async fn send_platform_message(&mut self, message: FeishuOutboundMessage) -> Result<()> {
        let target = FeishuSendTarget::parse(message.conversation_id()).with_context(|| {
            format!(
                "unsupported feishu conversation id: {}",
                message.conversation_id()
            )
        })?;
        let token = self.tenant_access_token().await?;
        let request = FeishuSendMessageRequest::from_outbound(target.receive_id.clone(), message)?;
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type={}",
            self.config.domain.trim_end_matches('/'),
            target.receive_id_type
        );
        let response = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(&request)
            .send()
            .await
            .context("send feishu message request")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("read feishu send message response")?;
        if !status.is_success() {
            return Err(anyhow!("feishu send message failed with {status}: {body}"));
        }
        let parsed: FeishuApiResponse<serde_json::Value> =
            serde_json::from_str(&body).context("parse feishu send message response")?;
        if parsed.code != 0 {
            return Err(anyhow!(
                "feishu send message rejected: code={} msg={}",
                parsed.code,
                parsed.msg
            ));
        }
        Ok(())
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
        let parsed: FeishuApiResponse<FeishuTenantTokenData> =
            serde_json::from_str(&body).context("parse feishu token response")?;
        if parsed.code != 0 {
            return Err(anyhow!(
                "feishu token request rejected: code={} msg={}",
                parsed.code,
                parsed.msg
            ));
        }
        let data = parsed
            .data
            .ok_or_else(|| anyhow!("feishu token response missing data"))?;
        let ttl = data.expire.saturating_sub(60) as u64;
        self.token_cache = Some(FeishuTokenCache {
            token: data.tenant_access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl.max(60)),
        });
        Ok(data.tenant_access_token)
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
            let root_id = if self.thread_isolation {
                payload.message.root_id.clone()
            } else {
                None
            };

            if let Some(inbound) =
                FeishuInboundMessage::from_event(sender_id, chat_id, message, content, root_id)
                    .map_err(|err| feishu_sdk::core::Error::InvalidEventFormat(err.to_string()))?
            {
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

impl FeishuSendMessageRequest {
    fn from_outbound(receive_id: String, message: FeishuOutboundMessage) -> Result<Self> {
        match message {
            FeishuOutboundMessage::Text { text, .. } => Ok(Self {
                receive_id,
                msg_type: "text",
                content: serde_json::to_string(&serde_json::json!({ "text": text }))?,
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

#[cfg(test)]
mod tests {
    use crate::adapter::feishu::FeishuPlatformEvent;

    #[test]
    fn feishu_event_text_message_maps_to_platform_event() {
        let message = crate::adapter::feishu::inbound::FeishuInboundMessage::from_event(
            "ou_user".to_string(),
            "oc_chat".to_string(),
            "text".to_string(),
            "{\"text\":\"hello\"}".to_string(),
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
}
