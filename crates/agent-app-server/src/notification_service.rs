use crate::command_router::ServerState;
use agent_protocol::{AppServerMessage, AppServerNotification, AppServerRequest};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub(crate) async fn send_notification(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    notification: AppServerNotification,
) {
    let subscribed = {
        let state = state.lock().await;
        state.is_subscribed(notification.conversation_id())
    };
    let message = AppServerMessage::Notification(notification);
    if subscribed {
        let _ = event_tx.send(message);
    }
}

pub(crate) async fn send_request(
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    request: AppServerRequest,
) {
    let subscribed = {
        let state = state.lock().await;
        state.is_subscribed(request.conversation_id())
    };
    let message = AppServerMessage::Request(request);
    if subscribed {
        let _ = event_tx.send(message);
    }
}
