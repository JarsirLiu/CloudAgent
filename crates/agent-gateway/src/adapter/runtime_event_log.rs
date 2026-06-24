use crate::gateway_event::GatewayEvent;
use tracing::{debug, info, warn};

pub(crate) fn log_outbound_events(
    session_key: &str,
    event_name: &str,
    outbounds: &[GatewayEvent],
    adapter_tag: &str,
) {
    if outbounds.is_empty() {
        debug!(
            session_key = %session_key,
            event = event_name,
            adapter = adapter_tag,
            "gateway.runtime.outbound.empty"
        );
        return;
    }

    for outbound in outbounds {
        match outbound {
            GatewayEvent::ItemDelta { kind, delta, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = ?kind,
                chars = delta.chars().count(),
                preview = %preview(delta, 120),
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ItemProgress {
                item_id, progress, ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "item_progress",
                item_id,
                progress = ?progress,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ItemMetricsUpdated {
                item_id, metrics, ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "item_metrics_updated",
                item_id,
                metrics = ?metrics,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ReasoningSummaryPartAdded {
                item_id,
                summary_index,
                ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "reasoning_summary_part_added",
                item_id,
                summary_index,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::TurnCompleted { .. } => info!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "turn_completed",
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ItemCompleted { item, .. } => info!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "item_completed",
                item = ?item,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ServerRequestRequested { request, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "server_request_requested",
                request = ?request,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ServerRequestResolved {
                request_id,
                decision,
                ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "server_request_resolved",
                request_id = ?request_id,
                decision = ?decision,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::TokenUsageUpdated {
                total_usage,
                model_context_window,
                ..
            } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "token_usage_updated",
                total_usage = ?total_usage,
                model_context_window = ?model_context_window,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ModelRetrying {
                stage,
                attempt,
                next_delay_ms,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "model_retrying",
                stage = ?stage,
                attempt,
                next_delay_ms,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ContextCompactionStarted {
                phase,
                estimated_tokens,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "context_compaction_started",
                phase = ?phase,
                estimated_tokens,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ContextCompacted {
                phase,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                ..
            } => info!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "context_compacted",
                phase = ?phase,
                pre_context_tokens_estimate,
                post_context_tokens_estimate,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::ItemStarted { item, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = ?item,
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::TurnStarted { turn_id, .. } => debug!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                turn_id,
                kind = "turn_started",
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::TurnFailed { error, .. } => warn!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "turn_failed",
                preview = %preview(error, 120),
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::TurnCancelled { reason, .. } => info!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "turn_cancelled",
                preview = %preview(reason, 120),
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::Info { message, .. } => info!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "info",
                preview = %preview(message, 120),
                "gateway.runtime.outbound.generated"
            ),
            GatewayEvent::Error { message, .. } => warn!(
                session_key = %session_key,
                event = event_name,
                adapter = adapter_tag,
                kind = "error",
                preview = %preview(message, 120),
                "gateway.runtime.outbound.generated"
            ),
        }
    }
}

pub(crate) fn preview(text: &str, max_chars: usize) -> String {
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
