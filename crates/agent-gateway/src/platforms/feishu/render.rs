use crate::gateway_outbound::{GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate};
use crate::message::OutboundMessage;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info};

const TOOL_NOTICE_COLLAPSE_WINDOW: Duration = Duration::from_secs(4);

#[derive(Default)]
pub struct FeishuOutboundRenderer {
    conversations: HashMap<String, FeishuConversationState>,
}

#[derive(Default)]
struct FeishuConversationState {
    text_buffer: String,
    tool_notice_count: usize,
    reasoning_announced: bool,
    last_tool_notice: Option<String>,
    last_tool_notice_at: Option<Instant>,
    collapsed_multi_tool_notice: bool,
    final_text_sent: bool,
    last_progress_at: Option<Instant>,
}

impl FeishuOutboundRenderer {
    pub fn render(&mut self, outbound: GatewayOutbound) -> Vec<OutboundMessage> {
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
                let is_group = is_group_context(&target);
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
                state.last_progress_at = None;
                info!(
                    conversation_id = %target.conversation_id,
                    text_chars = text.chars().count(),
                    text_preview = %preview(&text, 120),
                    "feishu.renderer.final_text.emit"
                );
                vec![OutboundMessage {
                    chat_id: target.chat_id,
                    text,
                    is_group_context: is_group,
                    reply_context: target.reply_context,
                }]
            }
            GatewayOutbound::Progress(progress) => self.render_progress(progress),
            GatewayOutbound::Info { target, message }
            | GatewayOutbound::Error { target, message } => {
                let is_group = is_group_context(&target);
                let summarized = summarize_for_feishu(&message, 220);
                info!(
                    conversation_id = %target.conversation_id,
                    source_chars = message.chars().count(),
                    rendered_chars = summarized.chars().count(),
                    truncated = summarized.chars().count() < message.split_whitespace().collect::<Vec<_>>().join(" ").trim().chars().count(),
                    "feishu.renderer.info.emit"
                );
                vec![OutboundMessage {
                    chat_id: target.chat_id,
                    text: summarized,
                    is_group_context: is_group,
                    reply_context: target.reply_context,
                }]
            }
        }
    }

    fn flush_buffer_as_final(
        &mut self,
        target: crate::gateway_outbound::OutboundTarget,
    ) -> Vec<OutboundMessage> {
        let Some(state) = self.conversations.get_mut(&target.conversation_id) else {
            info!(
                conversation_id = %target.conversation_id,
                "feishu.renderer.flush.suppressed_empty"
            );
            return Vec::new();
        };
        if state.final_text_sent {
            state.final_text_sent = false;
            info!(
                conversation_id = %target.conversation_id,
                "feishu.renderer.flush.suppressed_final_already_sent"
            );
            return Vec::new();
        }
        let text = state.text_buffer.trim().to_string();
        state.text_buffer.clear();
        state.tool_notice_count = 0;
        state.reasoning_announced = false;
        state.last_tool_notice = None;
        state.last_tool_notice_at = None;
        state.collapsed_multi_tool_notice = false;
        state.last_progress_at = None;
        if text.is_empty() {
            info!(
                conversation_id = %target.conversation_id,
                "feishu.renderer.flush.suppressed_empty"
            );
            return Vec::new();
        }
        info!(
            conversation_id = %target.conversation_id,
            text_chars = text.chars().count(),
            text_preview = %preview(&text, 120),
            "feishu.renderer.flush.emit"
        );
        let is_group = is_group_context(&target);
        vec![OutboundMessage {
            chat_id: target.chat_id,
            text,
            is_group_context: is_group,
            reply_context: target.reply_context,
        }]
    }

    fn render_progress(&mut self, progress: GatewayProgressUpdate) -> Vec<OutboundMessage> {
        let state = self
            .conversations
            .entry(progress.target.conversation_id.clone())
            .or_default();

        match progress.kind {
            GatewayProgressKind::Plan => {
                let _ = (state, progress);
                Vec::new()
            }
            GatewayProgressKind::Reasoning => {
                if progress.streaming {
                    if !state.reasoning_announced {
                        state.text_buffer.clear();
                        state.final_text_sent = false;
                        state.tool_notice_count = 0;
                        state.last_tool_notice = None;
                        state.last_tool_notice_at = None;
                        state.collapsed_multi_tool_notice = false;
                        state.reasoning_announced = true;
                        state.last_progress_at = Some(Instant::now());
                        let notice = "正在思考中...";
                        debug!(
                            conversation_id = %progress.target.conversation_id,
                            kind = "reasoning",
                            decision = "emit_streaming_notice",
                            "feishu.renderer.progress"
                        );
                        return vec![to_message(progress.target, notice.to_string())];
                    }
                    return Vec::new();
                }

                Vec::new()
            }
            GatewayProgressKind::Tool => {
                let summary = summarize_tool_notice(&progress.summary);
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
                    return vec![to_message(
                        progress.target,
                        "正在继续处理多个步骤...".to_string(),
                    )];
                }
                let max_tool_notices = 3;
                if state.tool_notice_count >= max_tool_notices {
                    return Vec::new();
                }
                state.tool_notice_count += 1;
                state.last_tool_notice = Some(summary.to_string());
                state.last_tool_notice_at = Some(now);
                vec![to_message(progress.target, summary.to_string())]
            }
        }
    }
}

fn is_group_context(target: &crate::gateway_outbound::OutboundTarget) -> bool {
    !matches!(target.chat_type.as_deref(), Some("p2p") | Some("dm") | None)
}

fn to_message(target: crate::gateway_outbound::OutboundTarget, text: String) -> OutboundMessage {
    let is_group = is_group_context(&target);
    OutboundMessage {
        chat_id: target.chat_id,
        text,
        is_group_context: is_group,
        reply_context: target.reply_context,
    }
}

fn append_text_delta(buffer: &mut String, delta: &str) {
    if delta.trim().is_empty() {
        return;
    }
    buffer.push_str(delta);
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

fn summarize_tool_notice(text: &str) -> &str {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "";
    }
    trimmed
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
mod tests {
    use super::FeishuOutboundRenderer;
    use crate::gateway_outbound::{
        GatewayOutbound, GatewayProgressKind, GatewayProgressUpdate, OutboundTarget,
    };

    fn target() -> OutboundTarget {
        OutboundTarget {
            conversation_id: "feishu:p2p:ou_1".to_string(),
            chat_id: "oc_123".to_string(),
            chat_type: Some("p2p".to_string()),
            is_reply_chain: false,
            reply_context: None,
        }
    }

    #[test]
    fn streaming_reasoning_only_announces_once() {
        let mut renderer = FeishuOutboundRenderer::default();
        let first = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Reasoning,
            summary: "thinking".to_string(),
            streaming: true,
        }));
        let second = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Reasoning,
            summary: "still thinking".to_string(),
            streaming: true,
        }));

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].text, "正在思考中...");
        assert!(second.is_empty());
    }

    #[test]
    fn final_text_is_delivered() {
        let mut renderer = FeishuOutboundRenderer::default();
        let messages = renderer.render(GatewayOutbound::FinalText {
            target: target(),
            text: "final".to_string(),
        });

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "final");
        assert_eq!(messages[0].chat_id, "oc_123");
    }

    #[test]
    fn flush_uses_full_accumulated_text() {
        let mut renderer = FeishuOutboundRenderer::default();
        renderer.render(GatewayOutbound::TextDelta {
            target: target(),
            delta: "hello ".to_string(),
        });
        let messages = renderer.render(GatewayOutbound::FlushText { target: target() });

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "hello");
    }

    #[test]
    fn flush_is_suppressed_after_final_text() {
        let mut renderer = FeishuOutboundRenderer::default();
        let target = target();
        let final_messages = renderer.render(GatewayOutbound::FinalText {
            target: target.clone(),
            text: "final".to_string(),
        });
        let flushed = renderer.render(GatewayOutbound::FlushText { target });

        assert_eq!(final_messages.len(), 1);
        assert!(flushed.is_empty());
    }

    #[test]
    fn tool_progress_is_suppressed() {
        let mut renderer = FeishuOutboundRenderer::default();
        let messages = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Tool,
            summary: "正在查看 Git 历史...".to_string(),
            streaming: false,
        }));

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "正在查看 Git 历史...");
    }

    #[test]
    fn group_context_keeps_only_light_reasoning_notice() {
        let mut renderer = FeishuOutboundRenderer::default();
        let target = OutboundTarget {
            conversation_id: "agent:main:feishu:group:oc_group:ou_1".to_string(),
            chat_id: "oc_group".to_string(),
            chat_type: Some("group".to_string()),
            is_reply_chain: false,
            reply_context: None,
        };

        let reasoning = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target.clone(),
            kind: GatewayProgressKind::Reasoning,
            summary: "thinking".to_string(),
            streaming: true,
        }));
        let tool = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target,
            kind: GatewayProgressKind::Tool,
            summary: "正在查看 Git 历史...".to_string(),
            streaming: false,
        }));

        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0].text, "正在思考中...");
        assert_eq!(tool.len(), 1);
        assert_eq!(tool[0].text, "正在查看 Git 历史...");
    }

    #[test]
    fn new_turn_reasoning_clears_stale_buffer() {
        let mut renderer = FeishuOutboundRenderer::default();
        let target = target();
        renderer.render(GatewayOutbound::TextDelta {
            target: target.clone(),
            delta: "stale".to_string(),
        });

        let _ = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target.clone(),
            kind: GatewayProgressKind::Reasoning,
            summary: "thinking".to_string(),
            streaming: true,
        }));

        renderer.render(GatewayOutbound::TextDelta {
            target: target.clone(),
            delta: "fresh".to_string(),
        });
        let flushed = renderer.render(GatewayOutbound::FlushText { target });

        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].text, "fresh");
    }

    #[test]
    fn completed_reasoning_is_suppressed() {
        let mut renderer = FeishuOutboundRenderer::default();
        let messages = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Reasoning,
            summary: "long reasoning summary".to_string(),
            streaming: false,
        }));

        assert!(messages.is_empty());
    }

    #[test]
    fn plan_progress_is_suppressed() {
        let mut renderer = FeishuOutboundRenderer::default();
        let messages = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Plan,
            summary: "draft plan".to_string(),
            streaming: true,
        }));

        assert!(messages.is_empty());
    }

    #[test]
    fn duplicate_tool_notice_is_suppressed() {
        let mut renderer = FeishuOutboundRenderer::default();
        let first = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Tool,
            summary: "正在查看 Git 历史...".to_string(),
            streaming: false,
        }));
        let second = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Tool,
            summary: "正在查看 Git 历史...".to_string(),
            streaming: false,
        }));

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }

    #[test]
    fn rapid_distinct_tool_notices_collapse_to_generic_followup() {
        let mut renderer = FeishuOutboundRenderer::default();
        let first = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Tool,
            summary: "正在查看 Git 历史...".to_string(),
            streaming: false,
        }));
        let second = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Tool,
            summary: "正在查看项目文件...".to_string(),
            streaming: false,
        }));

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].text, "正在继续处理多个步骤...");
    }

    #[test]
    fn file_change_notice_can_surface_specific_path() {
        let mut renderer = FeishuOutboundRenderer::default();
        let messages = renderer.render(GatewayOutbound::Progress(GatewayProgressUpdate {
            target: target(),
            kind: GatewayProgressKind::Tool,
            summary: "正在修改文件: crates/agent-gateway/src/app_server_mapping.rs".to_string(),
            streaming: false,
        }));

        assert_eq!(messages.len(), 1);
        assert!(messages[0].text.contains("app_server_mapping.rs"));
    }
}
