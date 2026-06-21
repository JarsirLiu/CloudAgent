use super::WecomOutboundRenderer;
use crate::gateway_event::{GatewayEvent, GatewayItemDeltaKind, OutboundTarget};
use agent_core::{RuntimeItem, TranscriptItem, TurnItemKind};

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
fn reasoning_notice_only_once_per_phase() {
    let mut renderer = WecomOutboundRenderer::default();
    let first = renderer.render(GatewayEvent::ItemStarted {
        target: target(),
        turn_id: "turn1".to_string(),
        item: RuntimeItem::started(
            "item1",
            None,
            TurnItemKind::Reasoning,
            Some("reasoning".to_string()),
        ),
    });
    let second = renderer.render(GatewayEvent::ItemDelta {
        target: target(),
        turn_id: "turn1".to_string(),
        item_id: "item1".to_string(),
        call_id: None,
        kind: GatewayItemDeltaKind::ReasoningText,
        segment_index: Some(0),
        delta: "thinking".to_string(),
    });
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].content, "正在思考中...");
    assert!(second.is_empty());
}

#[test]
fn completed_agent_message_emits_final_text() {
    let mut renderer = WecomOutboundRenderer::default();
    let transcript_item = TranscriptItem::AgentMessage {
        id: "msg1".to_string(),
        text: "final".to_string(),
    };
    let messages = renderer.render(GatewayEvent::ItemCompleted {
        target: target(),
        turn_id: "turn1".to_string(),
        item: RuntimeItem::completed(&transcript_item, None),
        transcript_item,
    });
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "final");
}
