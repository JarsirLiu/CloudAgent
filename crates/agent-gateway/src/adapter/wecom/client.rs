use super::config::WecomAdapterConfig;
use super::inbound::WecomInboundMessage;
use super::outbound::WecomOutboundMessage;
use anyhow::{Context, Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

const WECOM_WS_ENDPOINT: &str = "wss://openws.work.weixin.qq.com";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WecomPlatformEvent {
    Message(WecomInboundMessage),
}

#[derive(Clone, Debug)]
struct WecomInboundEnvelope {
    event: WecomPlatformEvent,
}

pub struct WecomPlatformClient {
    inbound_rx: mpsc::Receiver<WecomInboundEnvelope>,
    websocket_outbound_tx: mpsc::Sender<WecomOutboundMessage>,
}

impl WecomPlatformClient {
    pub fn new(config: WecomAdapterConfig) -> Result<Self> {
        config.validate()?;
        let (inbound_tx, inbound_rx) = mpsc::channel(128);
        let (outbound_tx, outbound_rx) = mpsc::channel(128);
        tokio::spawn(run_wecom_websocket(config, inbound_tx.clone(), outbound_rx));
        Ok(Self {
            inbound_rx,
            websocket_outbound_tx: outbound_tx,
        })
    }

    pub async fn next_platform_event(&mut self) -> Result<Option<WecomPlatformEvent>> {
        Ok(self.inbound_rx.recv().await.map(|envelope| envelope.event))
    }

    pub async fn send_platform_message(&mut self, message: WecomOutboundMessage) -> Result<()> {
        self.websocket_outbound_tx.send(message).await?;
        Ok(())
    }
}

#[derive(Default, Deserialize)]
struct WecomWsHeaders {
    #[serde(default)]
    req_id: String,
}

#[derive(Deserialize)]
struct WecomWsFrame {
    #[serde(default)]
    cmd: String,
    #[serde(default)]
    headers: WecomWsHeaders,
    #[serde(default)]
    body: Option<serde_json::Value>,
    #[serde(default)]
    errcode: Option<i32>,
    #[serde(default)]
    errmsg: Option<String>,
}

#[derive(Default, Deserialize)]
struct WecomWsFrom {
    #[serde(default)]
    userid: String,
}

#[derive(Default, Deserialize)]
struct WecomWsText {
    #[serde(default)]
    content: String,
}

#[derive(Default, Deserialize)]
struct WecomWsImage {
    #[serde(default)]
    url: String,
}

#[derive(Default, Deserialize)]
struct WecomWsFile {
    #[serde(default)]
    url: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct WecomWsMessageBody {
    #[serde(default)]
    chatid: String,
    #[serde(default)]
    chattype: String,
    #[serde(default)]
    msgtype: String,
    #[serde(default)]
    from: WecomWsFrom,
    #[serde(default)]
    text: Option<WecomWsText>,
    #[serde(default)]
    image: Option<WecomWsImage>,
    #[serde(default)]
    file: Option<WecomWsFile>,
}

async fn run_wecom_websocket(
    config: WecomAdapterConfig,
    inbound_tx: mpsc::Sender<WecomInboundEnvelope>,
    mut outbound_rx: mpsc::Receiver<WecomOutboundMessage>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        match run_wecom_websocket_connection(&config, &inbound_tx, &mut outbound_rx).await {
            Ok(()) => return,
            Err(error) => {
                warn!("wecom websocket disconnected: {error:#}");
                sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn run_wecom_websocket_connection(
    config: &WecomAdapterConfig,
    inbound_tx: &mpsc::Sender<WecomInboundEnvelope>,
    outbound_rx: &mut mpsc::Receiver<WecomOutboundMessage>,
) -> Result<()> {
    info!("connecting wecom websocket");
    let (stream, _) = connect_async(WECOM_WS_ENDPOINT)
        .await
        .context("connect wecom websocket")?;
    let (mut writer, mut reader) = stream.split();

    writer
        .send(Message::Text(
            json!({
                "cmd": "aibot_subscribe",
                "headers": { "req_id": "aibot_subscribe_1" },
                "body": {
                    "bot_id": config.bot_id,
                    "secret": config.bot_secret,
                }
            })
            .to_string(),
        ))
        .await
        .context("send wecom websocket subscribe")?;

    let ack = reader
        .next()
        .await
        .ok_or_else(|| anyhow!("wecom websocket closed before subscribe ack"))??;
    let ack = parse_ws_frame(ack)?;
    if ack.errcode.unwrap_or(0) != 0 {
        return Err(anyhow!(
            "wecom websocket subscribe failed: {}",
            ack.errmsg.unwrap_or_else(|| "unknown error".to_string())
        ));
    }
    info!("wecom websocket subscribed");

    let mut ping_seq = 1u64;
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            maybe_outbound = outbound_rx.recv() => {
                let Some(outbound) = maybe_outbound else {
                    return Ok(());
                };
                writer
                    .send(Message::Text(wecom_ws_outbound_frame(outbound)?))
                    .await
                    .context("send wecom websocket outbound")?;
            }
            _ = ping_interval.tick() => {
                ping_seq += 1;
                writer
                    .send(Message::Text(
                        json!({
                            "cmd": "ping",
                            "headers": { "req_id": format!("ping_{ping_seq}") }
                        }).to_string()
                    ))
                    .await
                    .context("send wecom websocket ping")?;
            }
            maybe_message = reader.next() => {
                let Some(message) = maybe_message else {
                    return Err(anyhow!("wecom websocket stream ended"));
                };
                let frame = parse_ws_frame(message?)?;
                if frame.cmd == "aibot_msg_callback" {
                    if let Some(envelope) = parse_ws_message(frame)? {
                        inbound_tx.send(envelope).await?;
                    }
                } else if frame.errcode.unwrap_or(0) != 0 {
                    warn!(
                        "wecom websocket frame error: req_id={} err={}",
                        frame.headers.req_id,
                        frame.errmsg.unwrap_or_else(|| "unknown error".to_string())
                    );
                } else {
                    debug!("wecom websocket frame received: cmd={}", frame.cmd);
                }
            }
        }
    }
}

fn parse_ws_frame(message: Message) -> Result<WecomWsFrame> {
    match message {
        Message::Text(text) => {
            serde_json::from_str(&text).context("parse wecom websocket text frame")
        }
        Message::Binary(binary) => {
            serde_json::from_slice(&binary).context("parse wecom websocket binary frame")
        }
        Message::Ping(_) | Message::Pong(_) => Ok(WecomWsFrame {
            cmd: String::new(),
            headers: WecomWsHeaders::default(),
            body: None,
            errcode: None,
            errmsg: None,
        }),
        Message::Close(frame) => Err(anyhow!(
            "wecom websocket closed: {}",
            frame
                .map(|frame| frame.reason.to_string())
                .unwrap_or_else(|| "no reason".to_string())
        )),
        Message::Frame(_) => Err(anyhow!("unexpected raw websocket frame")),
    }
}

fn parse_ws_message(frame: WecomWsFrame) -> Result<Option<WecomInboundEnvelope>> {
    let Some(body) = frame.body else {
        return Ok(None);
    };
    let body: WecomWsMessageBody =
        serde_json::from_value(body).context("parse wecom websocket callback body")?;
    let sender_id = body.from.userid;
    if sender_id.trim().is_empty() {
        return Ok(None);
    }

    let conversation_id = if body.chattype == "group" {
        format!("wecom:group:{}", body.chatid)
    } else {
        format!("wecom:single:{sender_id}")
    };

    let event = match body.msgtype.as_str() {
        "text" => WecomPlatformEvent::Message(WecomInboundMessage::Text {
            conversation_id,
            sender_id,
            text: body.text.unwrap_or_default().content,
        }),
        "image" => WecomPlatformEvent::Message(WecomInboundMessage::Image {
            conversation_id,
            sender_id,
            media_url: body.image.unwrap_or_default().url,
        }),
        "file" => {
            let file = body.file.unwrap_or_default();
            WecomPlatformEvent::Message(WecomInboundMessage::File {
                conversation_id,
                sender_id,
                media_url: file.url,
                file_name: file.name,
            })
        }
        _ => return Ok(None),
    };

    Ok(Some(WecomInboundEnvelope { event }))
}

fn wecom_ws_outbound_frame(message: WecomOutboundMessage) -> Result<String> {
    let conversation_id = message.conversation_id().to_string();
    let chat_id = conversation_id
        .strip_prefix("wecom:group:")
        .or_else(|| conversation_id.strip_prefix("wecom:single:"))
        .ok_or_else(|| anyhow!("unsupported wecom websocket conversation id: {conversation_id}"))?;
    let content = match message {
        WecomOutboundMessage::Text { text, .. } => text,
        WecomOutboundMessage::ApprovalCard { body, .. } => body,
    };
    Ok(json!({
        "cmd": "aibot_send_msg",
        "headers": {
            "req_id": format!("aibot_send_msg_{}", next_req_id()),
        },
        "body": {
            "chatid": chat_id,
            "msgtype": "markdown",
            "markdown": {
                "content": content,
            }
        }
    })
    .to_string())
}

fn next_req_id() -> u64 {
    static NEXT_REQ_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT_REQ_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::{WecomPlatformEvent, WecomWsFrame, WecomWsHeaders, parse_ws_message};
    use serde_json::json;

    #[test]
    fn wecom_ws_text_message_maps_to_platform_event() {
        let frame = WecomWsFrame {
            cmd: "aibot_msg_callback".to_string(),
            headers: WecomWsHeaders::default(),
            body: Some(json!({
                "msgtype": "text",
                "chattype": "single",
                "from": { "userid": "zhangsan" },
                "text": { "content": "hello" }
            })),
            errcode: None,
            errmsg: None,
        };

        match parse_ws_message(frame).expect("parse").expect("some").event {
            WecomPlatformEvent::Message(message) => {
                let gateway = message.into_gateway_message();
                assert_eq!(gateway.conversation_id, "wecom:single:zhangsan");
            }
        }
    }
}
