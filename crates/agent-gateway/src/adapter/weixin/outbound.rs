use crate::gateway_event::{GatewayEvent, GatewayItemDeltaKind, OutboundTarget};
use agent_core::{TranscriptItem, TurnItemKind};
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
    text_buffer: String,
    last_phase: Option<RenderPhase>,
    final_text_sent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderPhase {
    Reasoning,
    Tool,
}

impl WeixinOutboundRenderer {
    pub fn render(&mut self, event: GatewayEvent) -> Vec<WeixinOutboundMessage> {
        match event {
            GatewayEvent::TurnStarted { .. } => Vec::new(),
            GatewayEvent::ItemStarted {
                target,
                kind,
                title,
                ..
            } => self.render_item_started(target, kind, title),
            GatewayEvent::ItemDelta {
                target, kind, delta, ..
            } => self.render_item_delta(target, kind, delta),
            GatewayEvent::ItemCompleted { target, item, .. } => self.render_item_completed(target, item),
            GatewayEvent::TurnCompleted { target, .. } => self.flush_buffer(target),
            GatewayEvent::TurnFailed { target, error, .. } => {
                self.reset_state(&target.chat_id);
                vec![WeixinOutboundMessage {
                    chat_id: target.chat_id,
                    content: error,
                }]
            }
            GatewayEvent::TurnCancelled { target, reason, .. } => {
                self.reset_state(&target.chat_id);
                vec![WeixinOutboundMessage {
                    chat_id: target.chat_id,
                    content: format!("本轮已取消: {reason}"),
                }]
            }
            GatewayEvent::Info { target, message } | GatewayEvent::Error { target, message } => {
                vec![WeixinOutboundMessage {
                    chat_id: target.chat_id,
                    content: message,
                }]
            }
        }
    }

    fn render_item_started(
        &mut self,
        target: OutboundTarget,
        kind: TurnItemKind,
        _title: Option<String>,
    ) -> Vec<WeixinOutboundMessage> {
        match kind {
            TurnItemKind::Reasoning => self.enter_reasoning(target.chat_id),
            TurnItemKind::CommandExecution
            | TurnItemKind::FileChange
            | TurnItemKind::ToolCall
            | TurnItemKind::ToolResult => self.enter_tool(target.chat_id),
            TurnItemKind::AssistantMessage | TurnItemKind::UserMessage | TurnItemKind::SystemNote => {
                Vec::new()
            }
        }
    }

    fn render_item_delta(
        &mut self,
        target: OutboundTarget,
        kind: GatewayItemDeltaKind,
        delta: String,
    ) -> Vec<WeixinOutboundMessage> {
        match kind {
            GatewayItemDeltaKind::AgentMessage => {
                let state = self.state_mut(&target.chat_id);
                if state.final_text_sent && state.text_buffer.is_empty() {
                    state.final_text_sent = false;
                }
                if !delta.trim().is_empty() {
                    state.text_buffer.push_str(&delta);
                }
                Vec::new()
            }
            GatewayItemDeltaKind::ReasoningSummary | GatewayItemDeltaKind::ReasoningText => {
                self.enter_reasoning(target.chat_id)
            }
            GatewayItemDeltaKind::CommandExecutionOutput
            | GatewayItemDeltaKind::ToolOutput
            | GatewayItemDeltaKind::FileChangeOutput => Vec::new(),
            GatewayItemDeltaKind::Plan => Vec::new(),
        }
    }

    fn render_item_completed(
        &mut self,
        target: OutboundTarget,
        item: TranscriptItem,
    ) -> Vec<WeixinOutboundMessage> {
        match item {
            TranscriptItem::AgentMessage { text, .. } => {
                let state = self.state_mut(&target.chat_id);
                state.text_buffer.clear();
                state.final_text_sent = true;
                state.last_phase = None;
                vec![WeixinOutboundMessage {
                    chat_id: target.chat_id,
                    content: text,
                }]
            }
            TranscriptItem::Reasoning { .. } => Vec::new(),
            TranscriptItem::CommandExecution { .. }
            | TranscriptItem::FileChange { .. }
            | TranscriptItem::ToolResult { .. }
            | TranscriptItem::SystemMessage { .. }
            | TranscriptItem::UserMessage { .. } => Vec::new(),
        }
    }

    fn enter_reasoning(&mut self, chat_id: String) -> Vec<WeixinOutboundMessage> {
        let state = self.state_mut(&chat_id);
        if state.last_phase == Some(RenderPhase::Reasoning) {
            return Vec::new();
        }
        state.last_phase = Some(RenderPhase::Reasoning);
        vec![WeixinOutboundMessage {
            chat_id,
            content: "正在思考中...".to_string(),
        }]
    }

    fn enter_tool(&mut self, chat_id: String) -> Vec<WeixinOutboundMessage> {
        let state = self.state_mut(&chat_id);
        if state.last_phase == Some(RenderPhase::Tool) {
            return Vec::new();
        }
        state.last_phase = Some(RenderPhase::Tool);
        vec![WeixinOutboundMessage {
            chat_id,
            content: "正在调用工具处理中...".to_string(),
        }]
    }

    fn flush_buffer(&mut self, target: OutboundTarget) -> Vec<WeixinOutboundMessage> {
        let Some(state) = self.states.get_mut(&target.chat_id) else {
            return Vec::new();
        };
        if state.final_text_sent {
            state.final_text_sent = false;
            state.last_phase = None;
            return Vec::new();
        }
        let content = state.text_buffer.trim().to_string();
        state.text_buffer.clear();
        state.last_phase = None;
        if content.is_empty() {
            Vec::new()
        } else {
            vec![WeixinOutboundMessage {
                chat_id: target.chat_id,
                content,
            }]
        }
    }

    fn state_mut(&mut self, chat_id: &str) -> &mut RenderState {
        self.states.entry(chat_id.to_string()).or_default()
    }

    fn reset_state(&mut self, chat_id: &str) {
        self.states.remove(chat_id);
    }
}
