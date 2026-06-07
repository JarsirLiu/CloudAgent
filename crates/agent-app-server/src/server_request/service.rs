use crate::app::notification::{send_notification, send_request};
use crate::routing::command_router::ServerState;
use crate::session::conversation_watch::ConversationWatchManager;
use agent_core::AgentHost;
use agent_core::ServerRequestDecision;
use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest, RequestId};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub(crate) async fn resolve_command(
    runtime: &Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    view: &ConversationWatchManager,
    _conversation_id: String,
    request_id: RequestId,
    decision: ServerRequestDecision,
) {
    let resolved = {
        let mut state_guard = state.lock().await;
        state_guard.resolve_server_request(&request_id, decision)
    };
    if let Some(resolved) = resolved {
        runtime
            .resolve_pending_request(&resolved.conversation_id, &request_id)
            .await;
        view.note_server_request_resolved(&resolved.conversation_id, request_id.clone())
            .await;
        send_notification(
            event_tx,
            state,
            AppServerNotification::ServerRequestResolved {
                conversation_id: resolved.conversation_id,
                turn_id: resolved.turn_id,
                request_id,
                request: resolved.request,
                decision: resolved.decision,
            },
        )
        .await;
    }
}

pub(crate) async fn resolve_pending_for_finished_turn(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    view: &ConversationWatchManager,
    conversation_id: &str,
    turn_id: &str,
    reason: &str,
) {
    let resolved = {
        let mut state = state.lock().await;
        state.drain_server_requests_for_turn(
            turn_id,
            ServerRequestDecision::cancel(Some(reason.to_string())),
        )
    };
    for (request_id, turn_id, request, decision) in resolved {
        view.note_server_request_resolved(conversation_id, request_id.clone())
            .await;
        send_notification(
            event_tx,
            state,
            AppServerNotification::ServerRequestResolved {
                conversation_id: conversation_id.to_string(),
                turn_id,
                request_id,
                request,
                decision,
            },
        )
        .await;
    }
}

pub(crate) async fn resolve_pending_for_interrupted_conversation(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    view: &ConversationWatchManager,
    conversation_id: &str,
    reason: &str,
) {
    let resolved = {
        let mut state = state.lock().await;
        state.drain_server_requests_for_conversation(
            conversation_id,
            ServerRequestDecision::cancel(Some(reason.to_string())),
        )
    };
    for (request_id, turn_id, request, decision) in resolved {
        view.note_server_request_resolved(conversation_id, request_id.clone())
            .await;
        send_notification(
            event_tx,
            state,
            AppServerNotification::ServerRequestResolved {
                conversation_id: conversation_id.to_string(),
                turn_id,
                request_id,
                request,
                decision,
            },
        )
        .await;
    }
}

pub(crate) async fn replay_pending_for_conversation(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    conversation_id: &str,
) {
    let pending = {
        let state = state.lock().await;
        state.pending_server_requests_for_conversation(conversation_id)
    };
    for (request_id, request) in pending {
        send_request(
            event_tx,
            state,
            AppServerRequest::ServerRequest {
                request_id,
                conversation_id: conversation_id.to_string(),
                request,
            },
        )
        .await;
    }
}
