use crate::gateway_event::{GatewayEvent, GatewayItemDeltaKind, OutboundTarget};
use crate::message::OutboundMessage;
use agent_core::{RuntimeItem, TranscriptItem, TurnItemKind};
use std::collections::HashMap;
use tracing::info;

#[derive(Default)]
pub struct FeishuOutboundRenderer {
    conversations: HashMap<String, FeishuConversationState>,
}

#[derive(Default)]
struct FeishuConversationState {
    text_buffer: String,
    last_phase: Option<FeishuPhase>,
    final_text_sent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeishuPhase {
    Reasoning,
    Tool,
}

impl FeishuOutboundRenderer {
    pub fn render(&mut self, event: GatewayEvent) -> Vec<OutboundMessage> {
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
                self.single_message(target, summarize_for_feishu(&message, 220))
            }
        }
    }

    fn render_item_started(
        &mut self,
        target: OutboundTarget,
        item: RuntimeItem,
    ) -> Vec<OutboundMessage> {
        match item.kind {
            TurnItemKind::Reasoning => {
                self.enter_phase(target, FeishuPhase::Reasoning, "正在思考中...")
            }
            TurnItemKind::CommandExecution
            | TurnItemKind::FileChange
            | TurnItemKind::ToolCall
            | TurnItemKind::ToolResult => {
                self.enter_phase(target, FeishuPhase::Tool, "正在调用工具处理中...")
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
    ) -> Vec<OutboundMessage> {
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
                self.enter_phase(target, FeishuPhase::Reasoning, "正在思考中...")
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
    ) -> Vec<OutboundMessage> {
        match item {
            TranscriptItem::AgentMessage { text, .. } => {
                let state = self.state_mut(&target.conversation_id);
                state.text_buffer.clear();
                state.final_text_sent = true;
                state.last_phase = None;
                info!(
                    conversation_id = %target.conversation_id,
                    text_chars = text.chars().count(),
                    text_preview = %preview(&text, 120),
                    "feishu.renderer.final_text.emit"
                );
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
        phase: FeishuPhase,
        notice: &str,
    ) -> Vec<OutboundMessage> {
        let state = self.state_mut(&target.conversation_id);
        if state.last_phase == Some(phase) {
            return Vec::new();
        }
        state.last_phase = Some(phase);
        self.single_message(target, notice.to_string())
    }

    fn flush_buffer(&mut self, target: OutboundTarget) -> Vec<OutboundMessage> {
        let Some(state) = self.conversations.get_mut(&target.conversation_id) else {
            info!(
                conversation_id = %target.conversation_id,
                "feishu.renderer.flush.suppressed_empty"
            );
            return Vec::new();
        };
        if state.final_text_sent {
            state.final_text_sent = false;
            state.last_phase = None;
            info!(
                conversation_id = %target.conversation_id,
                "feishu.renderer.flush.suppressed_final_already_sent"
            );
            return Vec::new();
        }
        let text = state.text_buffer.trim().to_string();
        state.text_buffer.clear();
        state.last_phase = None;
        if text.is_empty() {
            info!(
                conversation_id = %target.conversation_id,
                "feishu.renderer.flush.suppressed_empty"
            );
            Vec::new()
        } else {
            info!(
                conversation_id = %target.conversation_id,
                text_chars = text.chars().count(),
                text_preview = %preview(&text, 120),
                "feishu.renderer.flush.emit"
            );
            self.single_message(target, text)
        }
    }

    fn single_message(&self, target: OutboundTarget, text: String) -> Vec<OutboundMessage> {
        vec![to_message(target, text)]
    }

    fn state_mut(&mut self, conversation_id: &str) -> &mut FeishuConversationState {
        self.conversations
            .entry(conversation_id.to_string())
            .or_default()
    }
}

fn is_group_context(target: &OutboundTarget) -> bool {
    !matches!(target.chat_type.as_deref(), Some("p2p") | Some("dm") | None)
}

fn to_message(target: OutboundTarget, text: String) -> OutboundMessage {
    let is_group = is_group_context(&target);
    OutboundMessage {
        chat_id: target.chat_id,
        text,
        is_group_context: is_group,
        reply_context: target.reply_context,
    }
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

fn preview(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out.replace('\n', "\\n")
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
