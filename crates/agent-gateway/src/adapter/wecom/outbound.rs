use crate::gateway_outbound::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate};
use crate::message::ReplyContext;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const MARKDOWN_LIMIT: usize = 3800;
const TOOL_NOTICE_COLLAPSE_WINDOW: Duration = Duration::from_secs(4);

#[derive(Debug, Clone)]
pub struct WecomOutboundMessage {
    pub chat_id: String,
    pub content: String,
    pub reply_context: Option<ReplyContext>,
}

#[derive(Default)]
pub struct WecomOutboundRenderer {
    conversations: HashMap<String, WecomConversationState>,
}

#[derive(Default)]
struct WecomConversationState {
    text_buffer: String,
    tool_notice_count: usize,
    reasoning_announced: bool,
    last_tool_notice: Option<String>,
    last_tool_notice_at: Option<Instant>,
    collapsed_multi_tool_notice: bool,
    final_text_sent: bool,
}

impl WecomOutboundRenderer {
    pub fn render(&mut self, outbound: GatewayOutbound) -> Vec<WecomOutboundMessage> {
        match outbound {
            GatewayOutbound::TextDelta { target, delta } => {
                let state = self
                    .conversations
                    .entry(target.conversation_id.clone())
                    .or_default();
                if state.final_text_sent && state.text_buffer.is_empty() {
                    state.final_text_sent = false;
                }
                append_text_delta(&mut state.text_buffer, &delta);
                Vec::new()
            }
            GatewayOutbound::FlushText { target } => self.flush_buffer_as_final(target),
            GatewayOutbound::FinalText { target, text } => {
                let state = self
                    .conversations
                    .entry(target.conversation_id.clone())
                    .or_default();
                state.text_buffer.clear();
                state.tool_notice_count = 0;
                state.reasoning_announced = false;
                state.last_tool_notice = None;
                state.last_tool_notice_at = None;
                state.collapsed_multi_tool_notice = false;
                state.final_text_sent = true;
                split_markdown_chunks(&text)
                    .into_iter()
                    .map(|content| WecomOutboundMessage {
                        chat_id: target.chat_id.clone(),
                        content,
                        reply_context: target.reply_context.clone(),
                    })
                    .collect()
            }
            GatewayOutbound::Progress(progress) => self.render_progress(progress),
            GatewayOutbound::Info { target, message }
            | GatewayOutbound::Error { target, message } => split_markdown_chunks(&message)
                .into_iter()
                .map(|content| WecomOutboundMessage {
                    chat_id: target.chat_id.clone(),
                    content,
                    reply_context: target.reply_context.clone(),
                })
                .collect(),
        }
    }

    fn flush_buffer_as_final(
        &mut self,
        target: crate::gateway_outbound::OutboundTarget,
    ) -> Vec<WecomOutboundMessage> {
        let Some(state) = self.conversations.get_mut(&target.conversation_id) else {
            return Vec::new();
        };
        if state.final_text_sent {
            state.final_text_sent = false;
            return Vec::new();
        }
        let text = state.text_buffer.trim().to_string();
        state.text_buffer.clear();
        state.tool_notice_count = 0;
        state.reasoning_announced = false;
        state.last_tool_notice = None;
        state.last_tool_notice_at = None;
        state.collapsed_multi_tool_notice = false;
        if text.is_empty() {
            return Vec::new();
        }
        split_markdown_chunks(&text)
            .into_iter()
            .map(|content| WecomOutboundMessage {
                chat_id: target.chat_id.clone(),
                content,
                reply_context: target.reply_context.clone(),
            })
            .collect()
    }

    fn render_progress(&mut self, progress: GatewayProgressUpdate) -> Vec<WecomOutboundMessage> {
        let state = self
            .conversations
            .entry(progress.target.conversation_id.clone())
            .or_default();

        match progress.kind {
            GatewayProgressKind::Plan => Vec::new(),
            GatewayProgressKind::Reasoning => {
                if progress.streaming && !state.reasoning_announced {
                    state.text_buffer.clear();
                    state.final_text_sent = false;
                    state.tool_notice_count = 0;
                    state.last_tool_notice = None;
                    state.last_tool_notice_at = None;
                    state.collapsed_multi_tool_notice = false;
                    state.reasoning_announced = true;
                    return vec![WecomOutboundMessage {
                        chat_id: progress.target.chat_id,
                        content: "正在思考中...".to_string(),
                        reply_context: progress.target.reply_context,
                    }];
                }
                Vec::new()
            }
            GatewayProgressKind::Tool => {
                let summary = progress.summary.trim();
                if summary.is_empty() {
                    return Vec::new();
                }
                if state
                    .last_tool_notice
                    .as_deref()
                    .is_some_and(|last| last == summary)
                {
                    return Vec::new();
                }
                let now = Instant::now();
                if state.tool_notice_count >= 1
                    && state
                        .last_tool_notice_at
                        .is_some_and(|last| now.duration_since(last) <= TOOL_NOTICE_COLLAPSE_WINDOW)
                    && !state.collapsed_multi_tool_notice
                {
                    state.tool_notice_count += 1;
                    state.collapsed_multi_tool_notice = true;
                    state.last_tool_notice = Some(summary.to_string());
                    state.last_tool_notice_at = Some(now);
                    return vec![WecomOutboundMessage {
                        chat_id: progress.target.chat_id,
                        content: "正在继续处理多个步骤...".to_string(),
                        reply_context: progress.target.reply_context,
                    }];
                }
                if state.tool_notice_count >= 3 {
                    return Vec::new();
                }
                state.tool_notice_count += 1;
                state.last_tool_notice = Some(summary.to_string());
                state.last_tool_notice_at = Some(now);
                vec![WecomOutboundMessage {
                    chat_id: progress.target.chat_id,
                    content: summary.to_string(),
                    reply_context: progress.target.reply_context,
                }]
            }
        }
    }
}

fn append_text_delta(buffer: &mut String, delta: &str) {
    if delta.trim().is_empty() {
        return;
    }
    buffer.push_str(delta);
}

fn split_markdown_chunks(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in trimmed.lines() {
        let candidate = if current.is_empty() {
            line.to_string()
        } else {
            format!("{current}\n{line}")
        };
        if candidate.chars().count() > MARKDOWN_LIMIT && !current.is_empty() {
            chunks.push(current);
            current = line.to_string();
        } else if candidate.chars().count() > MARKDOWN_LIMIT {
            let mut piece = String::new();
            for ch in line.chars() {
                if piece.chars().count() >= MARKDOWN_LIMIT {
                    chunks.push(piece);
                    piece = String::new();
                }
                piece.push(ch);
            }
            if !piece.is_empty() {
                chunks.push(piece);
            }
            current.clear();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::WecomOutboundRenderer;
    use crate::gateway_outbound::{
        GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate, OutboundTarget,
    };

    fn target() -> OutboundTarget {
        OutboundTarget {
            conversation_id: "agent:main:wecom:dm:u1".to_string(),
            chat_id: "chat1".to_string(),
            chat_type: Some("p2p".to_string()),
            is_reply_chain: false,
            reply_context: None,
        }
    }

    #[test]
    fn flush_uses_accumulated_text() {
        let mut renderer = WecomOutboundRenderer::default();
        renderer.render(GatewayOutbound::TextDelta {
            target: target(),
            delta: "hello ".to_string(),
        });
        let messages = renderer.render(GatewayOutbound::FlushText { target: target() });
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn reasoning_stream_notice_only_once() {
        let mut renderer = WecomOutboundRenderer::default();
        let first = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Reasoning,
            summary: "thinking".to_string(),
            streaming: true,
        }));
        let second = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Reasoning,
            summary: "thinking".to_string(),
            streaming: true,
        }));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].content, "正在思考中...");
        assert!(second.is_empty());
    }
}
