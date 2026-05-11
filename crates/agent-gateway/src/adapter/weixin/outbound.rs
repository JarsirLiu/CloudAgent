use crate::gateway_outbound::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WeixinOutboundMessage {
    pub chat_id: String,
    pub content: String,
}

#[derive(Default)]
pub struct WeixinOutboundRenderer {
    states: HashMap<String, RenderState>,
}

#[derive(Default)]
struct RenderState {
    buffer: String,
    last_phase: Option<RenderPhase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderPhase {
    Reasoning,
    Tool,
}

impl WeixinOutboundRenderer {
    pub fn render(&mut self, outbound: GatewayOutbound) -> Vec<WeixinOutboundMessage> {
        match outbound {
            GatewayOutbound::TextDelta { target, delta } => {
                self.state_mut(&target.chat_id).buffer.push_str(&delta);
                Vec::new()
            }
            GatewayOutbound::FlushText { target } => {
                self.flush_text(target.chat_id)
            }
            GatewayOutbound::FinalText { target, text } => {
                self.reset_state(&target.chat_id);
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
                let state = self.state_mut(&progress.target.chat_id);
                if state.last_phase == Some(RenderPhase::Reasoning) {
                    Vec::new()
                } else {
                    state.last_phase = Some(RenderPhase::Reasoning);
                    vec![WeixinOutboundMessage {
                        chat_id: progress.target.chat_id,
                        content: "正在思考中...".to_string(),
                    }]
                }
            }
            GatewayProgressKind::Tool => {
                let state = self.state_mut(&progress.target.chat_id);
                if state.last_phase == Some(RenderPhase::Tool) {
                    Vec::new()
                } else {
                    state.last_phase = Some(RenderPhase::Tool);
                    vec![WeixinOutboundMessage {
                        chat_id: progress.target.chat_id,
                        content: "正在调用工具处理中...".to_string(),
                    }]
                }
            }
        }
    }

    fn state_mut(&mut self, chat_id: &str) -> &mut RenderState {
        self.states.entry(chat_id.to_string()).or_default()
    }

    fn reset_state(&mut self, chat_id: &str) {
        self.states.remove(chat_id);
    }

    fn flush_text(&mut self, chat_id: String) -> Vec<WeixinOutboundMessage> {
        let Some(mut state) = self.states.remove(&chat_id) else {
            return Vec::new();
        };
        let content = state.buffer.trim().to_string();
        state.buffer.clear();
        if content.is_empty() {
            Vec::new()
        } else {
            vec![WeixinOutboundMessage { chat_id, content }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WeixinOutboundRenderer;
    use crate::gateway_outbound::{
        GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate, OutboundTarget,
    };

    #[test]
    fn flushes_text_per_chat_without_cross_talk() {
        let mut renderer = WeixinOutboundRenderer::default();

        renderer.render(GatewayOutbound::TextDelta {
            target: target("chat-a"),
            delta: "hello ".to_string(),
        });
        renderer.render(GatewayOutbound::TextDelta {
            target: target("chat-b"),
            delta: "world".to_string(),
        });
        renderer.render(GatewayOutbound::TextDelta {
            target: target("chat-a"),
            delta: "there".to_string(),
        });

        let flushed_a = renderer.render(GatewayOutbound::FlushText {
            target: target("chat-a"),
        });
        let flushed_b = renderer.render(GatewayOutbound::FlushText {
            target: target("chat-b"),
        });

        assert_eq!(flushed_a.len(), 1);
        assert_eq!(flushed_a[0].chat_id, "chat-a");
        assert_eq!(flushed_a[0].content, "hello there");
        assert_eq!(flushed_b.len(), 1);
        assert_eq!(flushed_b[0].chat_id, "chat-b");
        assert_eq!(flushed_b[0].content, "world");
    }

    #[test]
    fn announces_reasoning_per_chat() {
        let mut renderer = WeixinOutboundRenderer::default();

        let first_a = renderer.render(reasoning("chat-a", "thinking a"));
        let second_a = renderer.render(reasoning("chat-a", "thinking a again"));
        let first_b = renderer.render(reasoning("chat-b", "thinking b"));

        assert_eq!(first_a.len(), 1);
        assert_eq!(first_a[0].content, "正在思考中...");
        assert!(second_a.is_empty());
        assert_eq!(first_b.len(), 1);
        assert_eq!(first_b[0].chat_id, "chat-b");
        assert_eq!(first_b[0].content, "正在思考中...");
    }

    #[test]
    fn collapses_repeated_tool_progress_per_chat() {
        let mut renderer = WeixinOutboundRenderer::default();

        let first = renderer.render(tool("chat-a", "正在查看 Git 历史..."));
        let second = renderer.render(tool("chat-a", "正在查看代码改动..."));

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].content, "正在调用工具处理中...");
        assert!(second.is_empty());
    }

    #[test]
    fn reasoning_can_reappear_after_tool_phase_changes() {
        let mut renderer = WeixinOutboundRenderer::default();

        let _ = renderer.render(tool("chat-a", "正在查看 Git 历史..."));
        let reasoning = renderer.render(reasoning("chat-a", "模型开始处理当前消息"));

        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0].content, "正在思考中...");
    }

    fn reasoning(chat_id: &str, summary: &str) -> GatewayOutbound {
        GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(chat_id),
            kind: GatewayProgressKind::Reasoning,
            summary: summary.to_string(),
            streaming: true,
        })
    }

    fn tool(chat_id: &str, summary: &str) -> GatewayOutbound {
        GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(chat_id),
            kind: GatewayProgressKind::Tool,
            summary: summary.to_string(),
            streaming: true,
        })
    }

    fn target(chat_id: &str) -> OutboundTarget {
        OutboundTarget {
            conversation_id: format!("conversation-{chat_id}"),
            chat_id: chat_id.to_string(),
            chat_type: Some("dm".to_string()),
            is_reply_chain: false,
            reply_context: None,
        }
    }
}
