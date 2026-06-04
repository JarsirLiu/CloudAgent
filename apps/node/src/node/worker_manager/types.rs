use agent_app_server_client::TypedRequestError;
use agent_protocol::{AppClientCommand, CommandExecutionContext, JsonRpcRequest};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio::time::Instant;

#[derive(Default)]
pub(super) struct WorkerManagerState {
    pub(super) workers: HashMap<String, WorkerHandle>,
    pub(super) faults: HashMap<String, WorkerFaultRecord>,
}

#[derive(Clone, Debug)]
pub(crate) enum NodeEvent {
    Message {
        message: Box<agent_protocol::AppServerMessage>,
    },
    Diagnostic {
        conversation_id: String,
        message: String,
        is_error: bool,
    },
}

pub(super) struct WorkerHandle {
    pub(super) command_tx: mpsc::UnboundedSender<WorkerOutboundCommand>,
    pub(super) request_tx: mpsc::UnboundedSender<WorkerTypedRequest>,
    pub(super) events_tx: broadcast::Sender<NodeEvent>,
    pub(super) worker: tokio::task::JoinHandle<Result<()>>,
    pub(super) last_active_at: Instant,
}

pub(super) struct WorkerTypedRequest {
    pub(super) request: JsonRpcRequest,
    pub(super) context: Option<CommandExecutionContext>,
    pub(super) response_tx: oneshot::Sender<Result<serde_json::Value, TypedRequestError>>,
}

pub(super) struct WorkerOutboundCommand {
    pub(super) command: AppClientCommand,
    pub(super) context: Option<CommandExecutionContext>,
}

pub(super) struct WorkerFaultRecord {
    pub(super) detail: String,
    pub(super) failed_at_ms: u64,
}

pub(super) async fn record_worker_fault(
    state: &Arc<Mutex<WorkerManagerState>>,
    worker_scope_key: &str,
    detail: String,
) {
    let mut guard = state.lock().await;
    guard.faults.insert(
        worker_scope_key.to_string(),
        WorkerFaultRecord {
            detail,
            failed_at_ms: unix_timestamp_ms(),
        },
    );
}

pub(super) fn should_evict_worker(
    handle: &WorkerHandle,
    now: Instant,
    idle_worker_ttl: tokio::time::Duration,
) -> bool {
    handle.worker.is_finished() || now.duration_since(handle.last_active_at) >= idle_worker_ttl
}

fn unix_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
