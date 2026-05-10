use super::outbound::WecomOutboundMessage;
use crate::adapter::PlatformOutboundRenderer;
use crate::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate};
use std::collections::HashMap;

#[derive(Default)]
pub struct WecomOutboundRenderer {
    conversations: HashMap<String, WecomConversationState>,
}

#[derive(Default)]
struct WecomConversationState {
    preview_seen: bool,
    reasoning_sent: bool,
    tool_notice_count: usize,
}

impl PlatformOutboundRenderer for WecomOutboundRenderer {
    type Output = WecomOutboundMessage;

    fn render(&mut self, outbound: GatewayOutbound) -> Vec<Self::Output> {
        match outbound {
            GatewayOutbound::TextDelta {
                conversation_id,
                delta,
            } => {
                let state = self.conversations.entry(conversation_id).or_default();
                if !delta.trim().is_empty() {
                    state.preview_seen = true;
                }
                Vec::new()
            }
            GatewayOutbound::FlushText { conversation_id } => {
                let text = self
                    .conversations
                    .get(&conversation_id)
                    .map(|state| if state.preview_seen { "处理中结果已完成。".to_string() } else { String::new() })
                    .unwrap_or_default();
                self.conversations.remove(&conversation_id);
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![WecomOutboundMessage::Text { conversation_id, text }]
                }
            }
            GatewayOutbound::FinalText {
                conversation_id,
                text,
            } => {
                self.conversations.remove(&conversation_id);
                vec![WecomOutboundMessage::Text {
                    conversation_id,
                    text,
                }]
            }
            GatewayOutbound::Progress(progress) => self.render_progress(progress),
            other => vec![other.into()],
        }
    }
}

impl WecomOutboundRenderer {
    fn render_progress(&mut self, progress: GatewayProgressUpdate) -> Vec<WecomOutboundMessage> {
        let state = self
            .conversations
            .entry(progress.conversation_id.clone())
            .or_default();
        match progress.kind {
            GatewayProgressKind::Plan => {
                if progress.streaming {
                    return Vec::new();
                }
                vec![WecomOutboundMessage::Text {
                    conversation_id: progress.conversation_id,
                    text: format!("计划: {}", summarize_for_wecom(&progress.summary, 140)),
                }]
            }
            GatewayProgressKind::Reasoning => {
                if progress.streaming {
                    if state.reasoning_sent {
                        return Vec::new();
                    }
                    state.reasoning_sent = true;
                    return vec![WecomOutboundMessage::Text {
                        conversation_id: progress.conversation_id,
                        text: "正在思考中...".to_string(),
                    }];
                }
                Vec::new()
            }
            GatewayProgressKind::Tool => {
                if progress.streaming {
                    if state.preview_seen {
                        return Vec::new();
                    }
                    state.preview_seen = true;
                    return vec![WecomOutboundMessage::Text {
                        conversation_id: progress.conversation_id,
                        text: "正在调用工具处理中...".to_string(),
                    }];
                }
                if state.tool_notice_count >= 2 {
                    return Vec::new();
                }
                state.tool_notice_count += 1;
                vec![WecomOutboundMessage::Text {
                    conversation_id: progress.conversation_id,
                    text: format!("工具进度: {}", summarize_for_wecom(&progress.summary, 180)),
                }]
            }
        }
    }
}

fn summarize_for_wecom(text: &str, limit: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = flattened.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(limit).collect();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::WecomOutboundRenderer;
    use crate::adapter::PlatformOutboundRenderer;
    use crate::adapter::wecom::WecomOutboundMessage;
    use crate::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate};

    #[test]
    fn streaming_tool_only_announces_once() {
        let mut renderer = WecomOutboundRenderer::default();
        let first = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            conversation_id: "wecom:single:u1".to_string(),
            kind: GatewayProgressKind::Tool,
            summary: "running".to_string(),
            streaming: true,
        }));
        let second = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            conversation_id: "wecom:single:u1".to_string(),
            kind: GatewayProgressKind::Tool,
            summary: "running more".to_string(),
            streaming: true,
        }));

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }

    #[test]
    fn final_text_clears_state_and_sends_reply() {
        let mut renderer = WecomOutboundRenderer::default();
        let _ = renderer.render(GatewayOutbound::TextDelta {
            conversation_id: "wecom:single:u1".to_string(),
            delta: "partial".to_string(),
        });
        let messages = renderer.render(GatewayOutbound::FinalText {
            conversation_id: "wecom:single:u1".to_string(),
            text: "done".to_string(),
        });

        assert!(messages.iter().any(|msg| matches!(
            msg,
            WecomOutboundMessage::Text { text, .. } if text == "done"
        )));
    }
}
