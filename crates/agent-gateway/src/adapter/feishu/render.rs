use super::outbound::{FeishuOutboundMessage, FeishuProgressKind};
use crate::adapter::PlatformOutboundRenderer;
use crate::{GatewayOutbound, GatewayProgressUpdate};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct FeishuOutboundRenderer {
    conversations: HashMap<String, FeishuConversationState>,
}

#[derive(Default)]
struct FeishuConversationState {
    preview_buffer: String,
    preview_announced: bool,
    reasoning_announced: bool,
    tool_notice_count: usize,
    last_progress_at: Option<Instant>,
}

impl PlatformOutboundRenderer for FeishuOutboundRenderer {
    type Output = FeishuOutboundMessage;

    fn render(&mut self, outbound: GatewayOutbound) -> Vec<Self::Output> {
        match outbound {
            GatewayOutbound::TextDelta {
                conversation_id,
                delta,
            } => {
                let state = self.conversations.entry(conversation_id).or_default();
                append_preview_delta(&mut state.preview_buffer, &delta);
                Vec::new()
            }
            GatewayOutbound::FlushText { conversation_id } => {
                self.flush_preview_as_final(conversation_id)
            }
            GatewayOutbound::FinalText {
                conversation_id,
                text,
            } => {
                self.reset_state(&conversation_id);
                vec![FeishuOutboundMessage::Text {
                    conversation_id,
                    text,
                }]
            }
            GatewayOutbound::Progress(progress) => self.render_progress(progress),
            other => vec![other.into()],
        }
    }
}

impl FeishuOutboundRenderer {
    fn flush_preview_as_final(&mut self, conversation_id: String) -> Vec<FeishuOutboundMessage> {
        let text = self
            .conversations
            .get(&conversation_id)
            .map(|state| state.preview_buffer.trim().to_string())
            .unwrap_or_default();
        self.reset_state(&conversation_id);
        if text.is_empty() {
            return Vec::new();
        }
        vec![FeishuOutboundMessage::Text {
            conversation_id,
            text,
        }]
    }

    fn reset_state(&mut self, conversation_id: &str) {
        if let Some(state) = self.conversations.get_mut(conversation_id) {
            state.preview_buffer.clear();
            state.preview_announced = false;
            state.reasoning_announced = false;
            state.tool_notice_count = 0;
            state.last_progress_at = None;
        }
    }

    fn render_progress(&mut self, progress: GatewayProgressUpdate) -> Vec<FeishuOutboundMessage> {
        self.render_progress_message(
            progress.conversation_id,
            progress.kind.into(),
            progress.summary,
            progress.streaming,
        )
    }

    fn render_progress_message(
        &mut self,
        conversation_id: String,
        kind: FeishuProgressKind,
        summary: String,
        streaming: bool,
    ) -> Vec<FeishuOutboundMessage> {
        let state = self.conversations.entry(conversation_id.clone()).or_default();
        match kind {
            FeishuProgressKind::Plan => {
                if streaming {
                    if should_emit_progress(state, Duration::from_secs(8)) {
                        return vec![FeishuOutboundMessage::Text {
                            conversation_id,
                            text: "正在规划下一步。".to_string(),
                        }];
                    }
                    Vec::new()
                } else {
                    vec![FeishuOutboundMessage::Text {
                        conversation_id,
                        text: format!("计划: {}", summarize_for_feishu(&summary, 160)),
                    }]
                }
            }
            FeishuProgressKind::Reasoning => {
                if streaming {
                    if !state.reasoning_announced {
                        state.reasoning_announced = true;
                        state.last_progress_at = Some(Instant::now());
                        return vec![FeishuOutboundMessage::Text {
                            conversation_id,
                            text: "正在思考中...".to_string(),
                        }];
                    }
                    Vec::new()
                } else if should_emit_progress(state, Duration::from_secs(6)) {
                    vec![FeishuOutboundMessage::Text {
                        conversation_id,
                        text: format!("思路摘要: {}", summarize_for_feishu(&summary, 180)),
                    }]
                } else {
                    Vec::new()
                }
            }
            FeishuProgressKind::Tool => {
                if streaming {
                    if !state.preview_announced {
                        state.preview_announced = true;
                        state.last_progress_at = Some(Instant::now());
                        return vec![FeishuOutboundMessage::Text {
                            conversation_id,
                            text: "正在调用工具处理中...".to_string(),
                        }];
                    }
                    return Vec::new();
                }

                if state.tool_notice_count >= 3 {
                    return Vec::new();
                }
                state.tool_notice_count += 1;
                state.last_progress_at = Some(Instant::now());
                vec![FeishuOutboundMessage::Text {
                    conversation_id,
                    text: format!("工具进度: {}", summarize_for_feishu(&summary, 220)),
                }]
            }
        }
    }
}

fn should_emit_progress(state: &mut FeishuConversationState, min_interval: Duration) -> bool {
    let now = Instant::now();
    let should_emit = state
        .last_progress_at
        .map(|last| now.duration_since(last) >= min_interval)
        .unwrap_or(true);
    if should_emit {
        state.last_progress_at = Some(now);
    }
    should_emit
}

fn append_preview_delta(buffer: &mut String, delta: &str) {
    const LIMIT: usize = 240;
    if delta.trim().is_empty() || buffer.chars().count() >= LIMIT {
        return;
    }
    buffer.push_str(delta);
    let trimmed: String = buffer.chars().take(LIMIT).collect();
    *buffer = trimmed;
}

fn summarize_for_feishu(text: &str, limit: usize) -> String {
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
    use super::FeishuOutboundRenderer;
    use crate::adapter::PlatformOutboundRenderer;
    use crate::adapter::feishu::FeishuOutboundMessage;
    use crate::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate};

    #[test]
    fn streaming_reasoning_only_announces_once() {
        let mut renderer = FeishuOutboundRenderer::default();
        let first = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            conversation_id: "feishu:p2p:ou_1".to_string(),
            kind: GatewayProgressKind::Reasoning,
            summary: "thinking".to_string(),
            streaming: true,
        }));
        let second = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            conversation_id: "feishu:p2p:ou_1".to_string(),
            kind: GatewayProgressKind::Reasoning,
            summary: "still thinking".to_string(),
            streaming: true,
        }));

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }

    #[test]
    fn final_text_is_delivered_after_progress() {
        let mut renderer = FeishuOutboundRenderer::default();
        let _ = renderer.render(GatewayOutbound::TextDelta {
            conversation_id: "feishu:p2p:ou_1".to_string(),
            delta: "hello".to_string(),
        });
        let messages = renderer.render(GatewayOutbound::FinalText {
            conversation_id: "feishu:p2p:ou_1".to_string(),
            text: "final".to_string(),
        });

        assert!(!messages.is_empty());
        assert!(messages.iter().any(|msg| matches!(
            msg,
            FeishuOutboundMessage::Text { text, .. } if text == "final"
        )));
    }

    #[test]
    fn turn_completed_flushes_buffered_text() {
        let mut renderer = FeishuOutboundRenderer::default();
        let _ = renderer.render(GatewayOutbound::TextDelta {
            conversation_id: "feishu:p2p:ou_1".to_string(),
            delta: "buffered final".to_string(),
        });
        let messages = renderer.render(GatewayOutbound::FlushText {
            conversation_id: "feishu:p2p:ou_1".to_string(),
        });

        assert!(messages.iter().any(|msg| matches!(
            msg,
            FeishuOutboundMessage::Text { text, .. } if text == "buffered final"
        )));
    }
}
