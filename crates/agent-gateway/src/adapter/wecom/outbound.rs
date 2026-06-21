use crate::gateway_event::{GatewayEvent, GatewayItemDeltaKind, OutboundTarget};
use crate::message::ReplyContext;
use agent_core::{RuntimeItem, TranscriptItem, TurnItemKind};
use std::collections::HashMap;

const MARKDOWN_LIMIT: usize = 3800;

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
    last_phase: Option<WecomPhase>,
    final_text_sent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WecomPhase {
    Reasoning,
    Tool,
}

impl WecomOutboundRenderer {
    pub fn render(&mut self, event: GatewayEvent) -> Vec<WecomOutboundMessage> {
        match event {
            GatewayEvent::TurnStarted { .. } => Vec::new(),
            GatewayEvent::ItemStarted { target, item, .. } => {
                self.render_item_started(target, item)
            }
            GatewayEvent::ItemDelta {
                target,
                kind,
                delta,
                ..
            } => self.render_item_delta(target, kind, delta),
            GatewayEvent::ItemProgress { .. } | GatewayEvent::ItemMetricsUpdated { .. } => {
                Vec::new()
            }
            GatewayEvent::ReasoningSummaryPartAdded { .. } => Vec::new(),
            GatewayEvent::ItemCompleted {
                target,
                transcript_item,
                ..
            } => self.render_item_completed(target, transcript_item),
            GatewayEvent::ServerRequestRequested { .. }
            | GatewayEvent::ServerRequestResolved { .. }
            | GatewayEvent::TokenUsageUpdated { .. }
            | GatewayEvent::ModelRetrying { .. }
            | GatewayEvent::ContextCompactionStarted { .. }
            | GatewayEvent::ContextCompacted { .. } => Vec::new(),
            GatewayEvent::TurnCompleted { target, .. } => self.flush_buffer(target),
            GatewayEvent::TurnFailed { target, error, .. } => {
                self.conversations.remove(&target.conversation_id);
                self.single_message(target, error)
            }
            GatewayEvent::TurnCancelled { target, reason, .. } => {
                self.conversations.remove(&target.conversation_id);
                self.single_message(target, format!("本轮已取消: {reason}"))
            }
            GatewayEvent::Info { target, message } | GatewayEvent::Error { target, message } => {
                self.single_message(target, message)
            }
        }
    }

    fn render_item_started(
        &mut self,
        target: OutboundTarget,
        item: RuntimeItem,
    ) -> Vec<WecomOutboundMessage> {
        match item.kind {
            TurnItemKind::Reasoning => {
                self.enter_phase(target, WecomPhase::Reasoning, "正在思考中...")
            }
            TurnItemKind::CommandExecution
            | TurnItemKind::FileChange
            | TurnItemKind::ToolCall
            | TurnItemKind::ToolResult => {
                self.enter_phase(target, WecomPhase::Tool, "正在调用工具处理中...")
            }
            TurnItemKind::AssistantMessage
            | TurnItemKind::UserMessage
            | TurnItemKind::SystemNote => Vec::new(),
        }
    }

    fn render_item_delta(
        &mut self,
        target: OutboundTarget,
        kind: GatewayItemDeltaKind,
        delta: String,
    ) -> Vec<WecomOutboundMessage> {
        match kind {
            GatewayItemDeltaKind::AgentMessage => {
                let state = self.state_mut(&target.conversation_id);
                if state.final_text_sent && state.text_buffer.is_empty() {
                    state.final_text_sent = false;
                }
                if !delta.trim().is_empty() {
                    state.text_buffer.push_str(&delta);
                }
                Vec::new()
            }
            GatewayItemDeltaKind::ReasoningSummary | GatewayItemDeltaKind::ReasoningText => {
                self.enter_phase(target, WecomPhase::Reasoning, "正在思考中...")
            }
            GatewayItemDeltaKind::Plan
            | GatewayItemDeltaKind::CommandExecutionOutput
            | GatewayItemDeltaKind::ToolOutput
            | GatewayItemDeltaKind::JsonPatch => Vec::new(),
        }
    }

    fn render_item_completed(
        &mut self,
        target: OutboundTarget,
        item: TranscriptItem,
    ) -> Vec<WecomOutboundMessage> {
        match item {
            TranscriptItem::AgentMessage { text, .. } => {
                let state = self.state_mut(&target.conversation_id);
                state.text_buffer.clear();
                state.last_phase = None;
                state.final_text_sent = true;
                self.single_message(target, text)
            }
            TranscriptItem::Reasoning { .. }
            | TranscriptItem::CommandExecution { .. }
            | TranscriptItem::FileChange { .. }
            | TranscriptItem::ToolResult { .. }
            | TranscriptItem::SystemMessage { .. }
            | TranscriptItem::UserMessage { .. } => Vec::new(),
        }
    }

    fn enter_phase(
        &mut self,
        target: OutboundTarget,
        phase: WecomPhase,
        notice: &str,
    ) -> Vec<WecomOutboundMessage> {
        let state = self.state_mut(&target.conversation_id);
        if state.last_phase == Some(phase) {
            return Vec::new();
        }
        state.last_phase = Some(phase);
        self.single_message(target, notice.to_string())
    }

    fn flush_buffer(&mut self, target: OutboundTarget) -> Vec<WecomOutboundMessage> {
        let Some(state) = self.conversations.get_mut(&target.conversation_id) else {
            return Vec::new();
        };
        if state.final_text_sent {
            state.final_text_sent = false;
            state.last_phase = None;
            return Vec::new();
        }
        let text = state.text_buffer.trim().to_string();
        state.text_buffer.clear();
        state.last_phase = None;
        if text.is_empty() {
            Vec::new()
        } else {
            self.single_message(target, text)
        }
    }

    fn single_message(&self, target: OutboundTarget, text: String) -> Vec<WecomOutboundMessage> {
        split_markdown_chunks(&text)
            .into_iter()
            .map(|content| WecomOutboundMessage {
                chat_id: target.chat_id.clone(),
                content,
                reply_context: target.reply_context.clone(),
            })
            .collect()
    }

    fn state_mut(&mut self, conversation_id: &str) -> &mut WecomConversationState {
        self.conversations
            .entry(conversation_id.to_string())
            .or_default()
    }
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
#[path = "outbound_tests.rs"]
mod tests;
