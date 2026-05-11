use crate::gateway_outbound::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate};

#[derive(Debug, Clone)]
pub struct WeixinOutboundMessage {
    pub chat_id: String,
    pub content: String,
}

#[derive(Default)]
pub struct WeixinOutboundRenderer {
    buffer: String,
    announced_reasoning: bool,
}

impl WeixinOutboundRenderer {
    pub fn render(&mut self, outbound: GatewayOutbound) -> Vec<WeixinOutboundMessage> {
        match outbound {
            GatewayOutbound::TextDelta { target, delta } => {
                self.buffer.push_str(&delta);
                let _ = target;
                Vec::new()
            }
            GatewayOutbound::FlushText { target } => {
                if self.buffer.trim().is_empty() {
                    Vec::new()
                } else {
                    let content = std::mem::take(&mut self.buffer);
                    self.announced_reasoning = false;
                    vec![WeixinOutboundMessage {
                        chat_id: target.chat_id,
                        content,
                    }]
                }
            }
            GatewayOutbound::FinalText { target, text } => {
                self.buffer.clear();
                self.announced_reasoning = false;
                vec![WeixinOutboundMessage {
                    chat_id: target.chat_id,
                    content: text,
                }]
            }
            GatewayOutbound::Info { target, message } | GatewayOutbound::Error { target, message } => {
                vec![WeixinOutboundMessage {
                    chat_id: target.chat_id,
                    content: message,
                }]
            }
            GatewayOutbound::Progress(progress) => self.render_progress(progress),
        }
    }

    fn render_progress(&mut self, progress: GatewayProgressUpdate) -> Vec<WeixinOutboundMessage> {
        match progress.kind {
            GatewayProgressKind::Plan => Vec::new(),
            GatewayProgressKind::Reasoning => {
                if self.announced_reasoning {
                    Vec::new()
                } else {
                    self.announced_reasoning = true;
                    vec![WeixinOutboundMessage {
                        chat_id: progress.target.chat_id,
                        content: progress.summary,
                    }]
                }
            }
            GatewayProgressKind::Tool => vec![WeixinOutboundMessage {
                chat_id: progress.target.chat_id,
                content: progress.summary,
            }],
        }
    }
}
