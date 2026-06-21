use super::{EventFlow, map_app_server_event};
use crate::gateway_event::{GatewayEvent, OutboundTarget};
use agent_app_server_client::AppServerEvent;
use agent_core::{RuntimeItemMetrics, RuntimeItemProgress};
use agent_protocol::{AppServerMessage, AppServerNotification};

fn target() -> OutboundTarget {
    OutboundTarget {
        conversation_id: "default".to_string(),
        chat_id: "chat-1".to_string(),
        chat_type: None,
        is_reply_chain: false,
        reply_context: None,
    }
}

#[test]
fn item_progress_maps_to_gateway_event() {
    let event = AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::ItemProgress {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            progress: RuntimeItemProgress::message("weather seattle"),
        },
    ));

    let EventFlow::Continue(events) = map_app_server_event(&target(), event) else {
        panic!("expected continue flow");
    };
    assert!(matches!(
        &events[..],
        [GatewayEvent::ItemProgress {
            turn_id,
            item_id,
            call_id,
            progress,
            ..
        }] if turn_id == "turn-1"
            && item_id == "tool-1"
            && call_id.as_deref() == Some("call-1")
            && progress.message.as_deref() == Some("weather seattle")
    ));
}

#[test]
fn item_metrics_maps_to_gateway_event() {
    let event = AppServerEvent::Message(AppServerMessage::Notification(
        AppServerNotification::ItemMetricsUpdated {
            conversation_id: "default".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            metrics: RuntimeItemMetrics {
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                elapsed_ms: Some(42),
                bytes_read: None,
                bytes_written: None,
                file_count: Some(1),
                source_count: None,
                result_count: None,
            },
        },
    ));

    let EventFlow::Continue(events) = map_app_server_event(&target(), event) else {
        panic!("expected continue flow");
    };
    assert!(matches!(
        &events[..],
        [GatewayEvent::ItemMetricsUpdated {
            turn_id,
            item_id,
            call_id,
            metrics,
            ..
        }] if turn_id == "turn-1"
            && item_id == "tool-1"
            && call_id.as_deref() == Some("call-1")
            && metrics.elapsed_ms == Some(42)
            && metrics.file_count == Some(1)
    ));
}
