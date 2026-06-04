#[cfg(test)]
mod tests;
mod transport;
mod types;

use agent_app_server_client::{
    AppServerEvent, StdioAppServerClient, StdioClientConfig, TypedRequestError,
};
use agent_protocol::{
    AppClientCommand, CommandExecutionContext, JsonRpcRequest, NodeWorkerHealth, NodeWorkerStatus,
};
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio::time::{Duration, Instant, timeout};
use tracing::info;
pub(crate) use types::NodeEvent;
use types::{
    WorkerHandle, WorkerManagerState, WorkerOutboundCommand, WorkerTypedRequest,
    record_worker_fault, should_evict_worker,
};

const IDLE_WORKER_TTL: Duration = Duration::from_secs(300);
// Cold worker startup plus runtime selection can be noticeably slower on
// Windows debug builds, so typed reads need a less aggressive timeout.
const WORKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct WorkerManager {
    worker_program: OsString,
    data_root_dir: Option<OsString>,
    state: Arc<Mutex<WorkerManagerState>>,
}

impl WorkerManager {
    pub(crate) fn new(worker_program: OsString, data_root_dir: Option<OsString>) -> Self {
        Self {
            worker_program,
            data_root_dir,
            state: Arc::new(Mutex::new(WorkerManagerState::default())),
        }
    }

    pub(crate) async fn subscribe(
        &self,
        worker_scope_key: &str,
        _conversation_id: &str,
    ) -> Result<broadcast::Receiver<NodeEvent>> {
        self.prune_workers().await?;
        let mut state = self.state.lock().await;
        let handle = self.ensure_worker(&mut state, worker_scope_key).await?;
        Ok(handle.events_tx.subscribe())
    }

    pub(crate) async fn send_command(
        &self,
        worker_scope_key: &str,
        conversation_id: &str,
        command: AppClientCommand,
        context: Option<CommandExecutionContext>,
    ) -> Result<()> {
        self.prune_workers().await?;
        match self
            .send_command_inner(
                worker_scope_key,
                conversation_id,
                command.clone(),
                context.clone(),
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if error.to_string().contains("worker command channel closed") => {
                let mut state = self.state.lock().await;
                state.workers.remove(worker_scope_key);
                drop(state);
                self.send_command_inner(worker_scope_key, conversation_id, command, context)
                    .await
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn request_json(
        &self,
        worker_scope_key: &str,
        conversation_id: &str,
        request: JsonRpcRequest,
        context: Option<CommandExecutionContext>,
    ) -> Result<serde_json::Value, TypedRequestError> {
        self.prune_workers()
            .await
            .map_err(typed_request_transport_error(&request.method))?;
        let request_tx = {
            let mut state = self.state.lock().await;
            match self.ensure_worker(&mut state, worker_scope_key).await {
                Ok(handle) => {
                    handle.last_active_at = Instant::now();
                    handle.request_tx.clone()
                }
                Err(error) => {
                    return Err(typed_request_transport_error(&request.method)(error));
                }
            }
        };
        let (response_tx, response_rx) = oneshot::channel();
        request_tx
            .send(WorkerTypedRequest {
                request,
                context,
                response_tx,
            })
            .map_err(|_| TypedRequestError::Transport {
                method: "worker/request".to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    format!("worker request channel closed for {conversation_id}"),
                ),
            })?;
        response_rx
            .await
            .map_err(|_| TypedRequestError::Transport {
                method: "worker/request".to_string(),
                source: std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    format!("worker response channel closed for {conversation_id}"),
                ),
            })?
    }

    pub(crate) async fn is_worker_running(&self) -> bool {
        self.prune_workers().await.ok();
        !self.state.lock().await.workers.is_empty()
    }

    pub(crate) async fn status_snapshot(&self) -> Vec<NodeWorkerStatus> {
        self.prune_workers().await.ok();
        let now = Instant::now();
        let state = self.state.lock().await;
        let keys = state
            .workers
            .keys()
            .chain(state.faults.keys())
            .cloned()
            .collect::<BTreeSet<_>>();

        keys.into_iter()
            .filter_map(|worker_scope_key| {
                if let Some(handle) = state.workers.get(&worker_scope_key) {
                    Some(NodeWorkerStatus {
                        worker_scope_key,
                        health: NodeWorkerHealth::Running,
                        detail: None,
                        idle_for_ms: Some(
                            now.duration_since(handle.last_active_at).as_millis() as u64
                        ),
                        last_failure_at_ms: None,
                    })
                } else {
                    state
                        .faults
                        .get(&worker_scope_key)
                        .map(|fault| NodeWorkerStatus {
                            worker_scope_key,
                            health: NodeWorkerHealth::Faulted,
                            detail: Some(fault.detail.clone()),
                            idle_for_ms: None,
                            last_failure_at_ms: Some(fault.failed_at_ms),
                        })
                }
            })
            .collect()
    }

    pub(crate) async fn shutdown(&self) -> Result<()> {
        let evicted = {
            let mut state = self.state.lock().await;
            state
                .workers
                .drain()
                .map(|(_, handle)| handle)
                .collect::<Vec<_>>()
        };
        for handle in evicted {
            drop(handle.command_tx);
            handle.worker.await??;
        }
        Ok(())
    }

    async fn send_command_inner(
        &self,
        worker_scope_key: &str,
        conversation_id: &str,
        command: AppClientCommand,
        context: Option<CommandExecutionContext>,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let handle = self.ensure_worker(&mut state, worker_scope_key).await?;
        handle.last_active_at = Instant::now();
        if !matches!(&command, AppClientCommand::SubscribeConversation { .. }) {
            info!(
                worker_scope_key = %worker_scope_key,
                conversation_id = %conversation_id,
                command = %transport::worker_command_name(&command),
                "node.worker.command.send"
            );
        }
        handle
            .command_tx
            .send(WorkerOutboundCommand { command, context })
            .map_err(|_| anyhow!("worker command channel closed for {conversation_id}"))
    }

    async fn ensure_worker<'a>(
        &self,
        state: &'a mut WorkerManagerState,
        worker_scope_key: &str,
    ) -> Result<&'a mut WorkerHandle> {
        if !state.workers.contains_key(worker_scope_key) {
            state.faults.remove(worker_scope_key);
            let mut client = StdioAppServerClient::spawn(StdioClientConfig {
                program: self.worker_program.clone(),
                args: transport::worker_stdio_args(self.data_root_dir.clone()),
            })
            .await
            .context("failed to start worker instance")?;
            let (command_tx, mut command_rx) = mpsc::unbounded_channel::<WorkerOutboundCommand>();
            let (request_tx, mut request_rx) = mpsc::unbounded_channel::<WorkerTypedRequest>();
            let (events_tx, _) = broadcast::channel(256);
            let events_tx_for_worker = events_tx.clone();
            let state_for_worker = self.state.clone();
            let worker_scope_key = worker_scope_key.to_string();
            let worker_scope_key_for_task = worker_scope_key.clone();
            let worker = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        maybe_request = request_rx.recv() => {
                            if let Some(WorkerTypedRequest { request, context, response_tx, .. }) = maybe_request {
                                let method = request.method.clone();
                                let response = match timeout(
                                    WORKER_REQUEST_TIMEOUT,
                                    transport::request_worker_json(&client, request, context),
                                ).await {
                                    Ok(response) => response,
                                    Err(_) => Err(TypedRequestError::Transport {
                                        method,
                                        source: std::io::Error::new(
                                            std::io::ErrorKind::TimedOut,
                                            format!(
                                                "worker request timed out after {}s",
                                                WORKER_REQUEST_TIMEOUT.as_secs()
                                            ),
                                        ),
                                    }),
                                };
                                let fault_detail = response
                                    .as_ref()
                                    .err()
                                    .map(|error| error.to_string());
                                let _ = response_tx.send(response);
                                if let Some(detail) = fault_detail {
                                    record_worker_fault(
                                        &state_for_worker,
                                        &worker_scope_key_for_task,
                                        detail,
                                    ).await;
                                    let _ = client.shutdown().await;
                                    return Result::<()>::Ok(());
                                }
                            }
                        }
                        maybe_command = command_rx.recv() => {
                            match maybe_command {
                                Some(command) => client.send_command_with_context(command.command, command.context)?,
                                None => {
                                    client.shutdown().await?;
                                    return Result::<()>::Ok(());
                                }
                            }
                        }
                        maybe_event = client.next_event() => {
                            match maybe_event {
                                Some(AppServerEvent::Message(message)) => {
                                    let _ = events_tx_for_worker.send(NodeEvent::Message {
                                        message: Box::new(message),
                                    });
                                }
                                Some(AppServerEvent::Lagged { skipped }) => {
                                    info!(skipped, "node.worker.event.lagged");
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: "default".to_string(),
                                        message: format!("worker event channel lagged; skipped {skipped} events"),
                                        is_error: false,
                                    });
                                }
                                Some(AppServerEvent::Disconnected { message }) => {
                                    info!(
                                        message = %message,
                                        "node.worker.event.disconnected"
                                    );
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: "default".to_string(),
                                        message: transport::normalize_worker_disconnect_message(&message),
                                        is_error: true,
                                    });
                                    record_worker_fault(
                                        &state_for_worker,
                                        &worker_scope_key_for_task,
                                        transport::normalize_worker_disconnect_message(&message),
                                    ).await;
                                    return Result::<()>::Ok(());
                                }
                                None => {
                                    let detail = format!(
                                        "{} worker event stream ended unexpectedly",
                                        transport::ERR_TRANSPORT_CLOSED_PREFIX
                                    );
                                    info!("node.worker.event.stream_closed");
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: "default".to_string(),
                                        message: detail.clone(),
                                        is_error: true,
                                    });
                                    record_worker_fault(
                                        &state_for_worker,
                                        &worker_scope_key_for_task,
                                        detail,
                                    ).await;
                                    return Result::<()>::Ok(());
                                }
                            }
                        }
                    }
                }
            });
            state.workers.insert(
                worker_scope_key.to_string(),
                WorkerHandle {
                    command_tx,
                    request_tx,
                    events_tx,
                    worker,
                    last_active_at: Instant::now(),
                },
            );
        }
        state
            .workers
            .get_mut(worker_scope_key)
            .ok_or_else(|| anyhow!("worker handle missing"))
    }

    async fn prune_workers(&self) -> Result<()> {
        self.prune_workers_at(Instant::now()).await
    }

    async fn prune_workers_at(&self, now: Instant) -> Result<()> {
        let evicted = {
            let mut state = self.state.lock().await;
            let keys = state
                .workers
                .iter()
                .filter_map(|(key, handle)| {
                    if should_evict_worker(handle, now, IDLE_WORKER_TTL) {
                        Some(key.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let mut evicted = Vec::with_capacity(keys.len());
            for key in keys {
                if let Some(handle) = state.workers.remove(&key) {
                    evicted.push(handle);
                }
            }
            evicted
        };

        for handle in evicted {
            drop(handle.command_tx);
            handle.worker.await??;
        }
        Ok(())
    }
}

fn typed_request_transport_error(
    method: &str,
) -> impl FnOnce(anyhow::Error) -> TypedRequestError + '_ {
    move |error| TypedRequestError::Transport {
        method: method.to_string(),
        source: std::io::Error::other(error.to_string()),
    }
}
