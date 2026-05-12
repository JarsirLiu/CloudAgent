use agent_app_server_client::{
    AppServerEvent, StdioAppServerClient, StdioClientConfig, TypedRequestError,
};
use agent_protocol::{AppClientCommand, JsonRpcRequest, NodeWorkerHealth, NodeWorkerStatus};
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio::time::{Duration, Instant, timeout};
use tracing::info;

const IDLE_WORKER_TTL: Duration = Duration::from_secs(300);
const WORKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const ERR_TRANSPORT_CLOSED_PREFIX: &str = "ERR_TRANSPORT_CLOSED:";

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
    ) -> Result<()> {
        self.prune_workers().await?;
        match self
            .send_command_inner(worker_scope_key, conversation_id, command.clone())
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if error.to_string().contains("worker command channel closed") => {
                let mut state = self.state.lock().await;
                state.workers.remove(worker_scope_key);
                drop(state);
                self.send_command_inner(worker_scope_key, conversation_id, command)
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
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let handle = self.ensure_worker(&mut state, worker_scope_key).await?;
        handle.last_active_at = Instant::now();
        if !matches!(&command, AppClientCommand::SubscribeConversation { .. }) {
            info!(
                worker_scope_key = %worker_scope_key,
                conversation_id = %conversation_id,
                command = %worker_command_name(&command),
                "node.worker.command.send"
            );
        }
        handle
            .command_tx
            .send(command)
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
                args: worker_stdio_args(self.data_root_dir.clone()),
            })
            .await
            .context("failed to start scoped worker")?;
            let (command_tx, mut command_rx) = mpsc::unbounded_channel();
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
                            if let Some(WorkerTypedRequest { request, response_tx, .. }) = maybe_request {
                                let method = request.method.clone();
                                let response = match timeout(
                                    WORKER_REQUEST_TIMEOUT,
                                    client.request_typed::<serde_json::Value>(request),
                                ).await {
                                    Ok(response) => response,
                                    Err(_) => Err(TypedRequestError::Transport {
                                        method,
                                        source: std::io::Error::new(
                                            std::io::ErrorKind::TimedOut,
                                            format!(
                                                "scoped worker request timed out after {}s",
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
                                Some(command) => client.send_command(command)?,
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
                                        message: format!("scoped worker event channel lagged; skipped {skipped} events"),
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
                                        message: normalize_worker_disconnect_message(&message),
                                        is_error: true,
                                    });
                                    record_worker_fault(
                                        &state_for_worker,
                                        &worker_scope_key_for_task,
                                        normalize_worker_disconnect_message(&message),
                                    ).await;
                                    return Result::<()>::Ok(());
                                }
                                None => {
                                    let detail = format!(
                                        "{ERR_TRANSPORT_CLOSED_PREFIX} scoped worker event stream ended unexpectedly"
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
            .ok_or_else(|| anyhow!("scoped worker handle missing"))
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
                    if should_evict_worker(handle, now) {
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

fn worker_command_name(command: &AppClientCommand) -> &'static str {
    match command {
        AppClientCommand::SubmitTurn(_) => "submit_turn",
        AppClientCommand::ResolveServerRequest { .. } => "resolve_server_request",
        AppClientCommand::InterruptTurn { .. } => "interrupt_turn",
        AppClientCommand::CompactConversation { .. } => "compact_conversation",
        AppClientCommand::ResetConversation { .. } => "reset_conversation",
        AppClientCommand::RequestConversationStatus { .. } => "request_conversation_status",
        AppClientCommand::RequestConversationHistory { .. } => "request_conversation_history",
        AppClientCommand::RequestConversationHistoryPage { .. } => {
            "request_conversation_history_page"
        }
        AppClientCommand::ListConversations => "list_conversations",
        AppClientCommand::ListOnlineNodes => "list_online_nodes",
        AppClientCommand::ListPlatforms => "list_platforms",
        AppClientCommand::GetNodeStatus => "get_node_status",
        AppClientCommand::StopNode => "stop_node",
        AppClientCommand::SetConversationTitle { .. } => "set_conversation_title",
        AppClientCommand::CreateConversation { .. } => "create_conversation",
        AppClientCommand::SwitchConversation { .. } => "switch_conversation",
        AppClientCommand::SelectTargetNode { .. } => "select_target_node",
        AppClientCommand::GetPlatformStatus { .. } => "get_platform_status",
        AppClientCommand::GetPlatformConfig { .. } => "get_platform_config",
        AppClientCommand::SetPlatformEnabled { .. } => "set_platform_enabled",
        AppClientCommand::SetPlatformConfigValue { .. } => "set_platform_config_value",
        AppClientCommand::ClearPlatformConfigValue { .. } => "clear_platform_config_value",
        AppClientCommand::StartWeixinLogin => "start_weixin_login",
        AppClientCommand::CheckWeixinLogin { .. } => "check_weixin_login",
        AppClientCommand::ArchiveConversation { .. } => "archive_conversation",
        AppClientCommand::DeleteConversation { .. } => "delete_conversation",
        AppClientCommand::SubscribeConversation { .. } => "subscribe_conversation",
        AppClientCommand::UnsubscribeConversation { .. } => "unsubscribe_conversation",
        AppClientCommand::Exit => "exit",
    }
}

#[derive(Default)]
struct WorkerManagerState {
    workers: HashMap<String, WorkerHandle>,
    faults: HashMap<String, WorkerFaultRecord>,
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

struct WorkerHandle {
    command_tx: mpsc::UnboundedSender<AppClientCommand>,
    request_tx: mpsc::UnboundedSender<WorkerTypedRequest>,
    events_tx: broadcast::Sender<NodeEvent>,
    worker: tokio::task::JoinHandle<Result<()>>,
    last_active_at: Instant,
}

struct WorkerTypedRequest {
    request: JsonRpcRequest,
    response_tx: oneshot::Sender<Result<serde_json::Value, TypedRequestError>>,
}

struct WorkerFaultRecord {
    detail: String,
    failed_at_ms: u64,
}

fn typed_request_transport_error(
    method: &str,
) -> impl FnOnce(anyhow::Error) -> TypedRequestError + '_ {
    move |error| TypedRequestError::Transport {
        method: method.to_string(),
        source: std::io::Error::other(error.to_string()),
    }
}

fn should_evict_worker(handle: &WorkerHandle, now: Instant) -> bool {
    handle.worker.is_finished() || now.duration_since(handle.last_active_at) >= IDLE_WORKER_TTL
}

async fn record_worker_fault(
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

fn unix_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn normalize_worker_disconnect_message(message: &str) -> String {
    match message.trim() {
        "stdio app server closed" => {
            format!("{ERR_TRANSPORT_CLOSED_PREFIX} scoped worker app server closed unexpectedly")
        }
        other => format!("{ERR_TRANSPORT_CLOSED_PREFIX} {other}"),
    }
}

fn worker_stdio_args(data_root_dir: Option<OsString>) -> Vec<OsString> {
    let mut args = vec![OsString::from("app-server-stdio")];
    if let Some(data_root_dir) = data_root_dir {
        args.push(OsString::from("--data-dir"));
        args.push(data_root_dir);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::{
        IDLE_WORKER_TTL, NodeEvent, WorkerHandle, WorkerManager,
        normalize_worker_disconnect_message, record_worker_fault, should_evict_worker,
        worker_stdio_args,
    };
    use agent_protocol::NodeWorkerHealth;
    use anyhow::Result;
    use std::ffi::OsString;
    use tokio::sync::{broadcast, mpsc};
    use tokio::time::Instant;

    #[test]
    fn builds_shared_worker_stdio_arguments() {
        assert_eq!(
            worker_stdio_args(None),
            vec![OsString::from("app-server-stdio"),]
        );
    }

    #[test]
    fn worker_stdio_arguments_include_data_root_when_present() {
        assert_eq!(
            worker_stdio_args(Some(OsString::from("D:\\cloudagent-data"))),
            vec![
                OsString::from("app-server-stdio"),
                OsString::from("--data-dir"),
                OsString::from("D:\\cloudagent-data"),
            ]
        );
    }

    #[test]
    fn normalizes_worker_disconnect_messages() {
        assert_eq!(
            normalize_worker_disconnect_message("stdio app server closed"),
            "ERR_TRANSPORT_CLOSED: scoped worker app server closed unexpectedly"
        );
        assert_eq!(
            normalize_worker_disconnect_message("local node closed"),
            "ERR_TRANSPORT_CLOSED: local node closed"
        );
    }

    #[tokio::test]
    async fn prune_finished_worker_removes_completed_handle() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"), None);
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let worker = tokio::spawn(async { Result::<()>::Ok(()) });
        {
            let mut state = manager.state.lock().await;
            let (events_tx, _) = broadcast::channel(8);
            state.workers.insert(
                "session-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
                    request_tx: mpsc::unbounded_channel().0,
                    events_tx,
                    worker,
                    last_active_at: Instant::now(),
                },
            );
        }

        tokio::task::yield_now().await;
        manager.prune_workers().await?;
        assert!(manager.state.lock().await.workers.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn subscribe_receives_broadcast_events_from_existing_shared_worker() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"), None);
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let worker = tokio::spawn(async {
            std::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Result::<()>::Ok(())
        });
        let (events_tx, _) = broadcast::channel(8);
        {
            let mut state = manager.state.lock().await;
            state.workers.insert(
                "session-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
                    request_tx: mpsc::unbounded_channel().0,
                    events_tx: events_tx.clone(),
                    worker,
                    last_active_at: Instant::now(),
                },
            );
        }

        let mut receiver = manager.subscribe("session-1", "conversation-1").await?;
        let _ = events_tx.send(NodeEvent::Diagnostic {
            conversation_id: "default".to_string(),
            message: "hello".to_string(),
            is_error: false,
        });

        match receiver.recv().await? {
            NodeEvent::Diagnostic {
                conversation_id,
                message,
                is_error,
            } => {
                assert_eq!(conversation_id, "default");
                assert_eq!(message, "hello");
                assert!(!is_error);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn prune_worker_evicts_idle_handle() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"), None);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let worker = tokio::spawn(async move {
            while rx.recv().await.is_some() {}
            Result::<()>::Ok(())
        });
        {
            let mut state = manager.state.lock().await;
            let (events_tx, _) = broadcast::channel(8);
            state.workers.insert(
                "session-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
                    request_tx: mpsc::unbounded_channel().0,
                    events_tx,
                    worker,
                    last_active_at: Instant::now() - IDLE_WORKER_TTL,
                },
            );
        }

        manager.prune_workers_at(Instant::now()).await?;
        assert!(manager.state.lock().await.workers.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn prune_worker_keeps_recent_handle() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"), None);
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let worker = tokio::spawn(async {
            std::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Result::<()>::Ok(())
        });
        {
            let mut state = manager.state.lock().await;
            let (events_tx, _) = broadcast::channel(8);
            state.workers.insert(
                "session-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
                    request_tx: mpsc::unbounded_channel().0,
                    events_tx,
                    worker,
                    last_active_at: Instant::now(),
                },
            );
            let handle = state.workers.get("session-1").expect("worker handle");
            assert!(!should_evict_worker(handle, Instant::now()));
        }

        manager.prune_workers_at(Instant::now()).await?;
        assert!(manager.state.lock().await.workers.contains_key("session-1"));
        Ok(())
    }

    #[tokio::test]
    async fn status_snapshot_reports_running_and_faulted_scopes() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"), None);
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let worker = tokio::spawn(async {
            std::future::pending::<()>().await;
            #[allow(unreachable_code)]
            Result::<()>::Ok(())
        });
        {
            let mut state = manager.state.lock().await;
            let (events_tx, _) = broadcast::channel(8);
            state.workers.insert(
                "local:cli".to_string(),
                WorkerHandle {
                    command_tx: tx,
                    request_tx: mpsc::unbounded_channel().0,
                    events_tx,
                    worker,
                    last_active_at: Instant::now(),
                },
            );
        }
        record_worker_fault(&manager.state, "im:feishu", "transport failed".to_string()).await;

        let snapshot = manager.status_snapshot().await;

        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.iter().any(|status| {
            status.worker_scope_key == "local:cli"
                && matches!(status.health, NodeWorkerHealth::Running)
        }));
        assert!(snapshot.iter().any(|status| {
            status.worker_scope_key == "im:feishu"
                && matches!(status.health, NodeWorkerHealth::Faulted)
                && status.detail.as_deref() == Some("transport failed")
        }));
        Ok(())
    }
}
