use agent_app_server_client::{
    AppServerEvent, StdioAppServerClient, StdioClientConfig, TypedRequestError,
};
use agent_protocol::{AppClientCommand, JsonRpcRequest};
use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio::time::{Duration, Instant};

const IDLE_WORKER_TTL: Duration = Duration::from_secs(300);
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
        _conversation_id: &str,
    ) -> Result<broadcast::Receiver<NodeEvent>> {
        self.prune_worker().await?;
        let mut state = self.state.lock().await;
        let handle = self.ensure_worker(&mut state).await?;
        Ok(handle.events_tx.subscribe())
    }

    pub(crate) async fn send_command(
        &self,
        conversation_id: &str,
        command: AppClientCommand,
    ) -> Result<()> {
        self.prune_worker().await?;
        match self
            .send_command_inner(conversation_id, command.clone())
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if error.to_string().contains("worker command channel closed") => {
                let mut state = self.state.lock().await;
                state.worker = None;
                drop(state);
                self.send_command_inner(conversation_id, command).await
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn request_json(
        &self,
        conversation_id: &str,
        request: JsonRpcRequest,
    ) -> Result<serde_json::Value, TypedRequestError> {
        self.prune_worker()
            .await
            .map_err(typed_request_transport_error(&request.method))?;
        let request_tx = {
            let mut state = self.state.lock().await;
            match self.ensure_worker(&mut state).await {
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

    async fn send_command_inner(
        &self,
        conversation_id: &str,
        command: AppClientCommand,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let handle = self.ensure_worker(&mut state).await?;
        handle.last_active_at = Instant::now();
        handle
            .command_tx
            .send(command)
            .map_err(|_| anyhow!("worker command channel closed for {conversation_id}"))
    }

    async fn ensure_worker<'a>(
        &self,
        state: &'a mut WorkerManagerState,
    ) -> Result<&'a mut WorkerHandle> {
        if state.worker.is_none() {
            let mut client = StdioAppServerClient::spawn(StdioClientConfig {
                program: self.worker_program.clone(),
                args: worker_stdio_args(self.data_root_dir.clone()),
            })
            .await
            .context("failed to start shared worker")?;
            let (command_tx, mut command_rx) = mpsc::unbounded_channel();
            let (request_tx, mut request_rx) = mpsc::unbounded_channel::<WorkerTypedRequest>();
            let (events_tx, _) = broadcast::channel(256);
            let events_tx_for_worker = events_tx.clone();
            let worker = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        maybe_request = request_rx.recv() => {
                            match maybe_request {
                                Some(WorkerTypedRequest { request, response_tx, .. }) => {
                                    let response = client.request_typed::<serde_json::Value>(request).await;
                                    let _ = response_tx.send(response);
                                }
                                None => {}
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
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: "default".to_string(),
                                        message: format!("shared worker event channel lagged; skipped {skipped} events"),
                                        is_error: false,
                                    });
                                }
                                Some(AppServerEvent::Disconnected { message }) => {
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: "default".to_string(),
                                        message: normalize_worker_disconnect_message(&message),
                                        is_error: true,
                                    });
                                    return Result::<()>::Ok(());
                                }
                                None => {
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: "default".to_string(),
                                        message: format!(
                                            "{ERR_TRANSPORT_CLOSED_PREFIX} shared worker event stream ended unexpectedly"
                                        ),
                                        is_error: true,
                                    });
                                    return Result::<()>::Ok(());
                                }
                            }
                        }
                    }
                }
            });
            state.worker = Some(WorkerHandle {
                command_tx,
                request_tx,
                events_tx,
                worker,
                last_active_at: Instant::now(),
            });
        }
        state
            .worker
            .as_mut()
            .ok_or_else(|| anyhow!("shared worker handle missing"))
    }

    async fn prune_worker(&self) -> Result<()> {
        self.prune_worker_at(Instant::now()).await
    }

    async fn prune_worker_at(&self, now: Instant) -> Result<()> {
        let evicted = {
            let mut state = self.state.lock().await;
            if state
                .worker
                .as_ref()
                .is_some_and(|handle| should_evict_worker(handle, now))
            {
                state.worker.take()
            } else {
                None
            }
        };

        if let Some(handle) = evicted {
            drop(handle.command_tx);
            handle.worker.await??;
        }
        Ok(())
    }
}

#[derive(Default)]
struct WorkerManagerState {
    worker: Option<WorkerHandle>,
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

fn normalize_worker_disconnect_message(message: &str) -> String {
    match message.trim() {
        "stdio app server closed" => {
            format!("{ERR_TRANSPORT_CLOSED_PREFIX} shared worker app server closed unexpectedly")
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
        normalize_worker_disconnect_message, should_evict_worker, worker_stdio_args,
    };
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
            "ERR_TRANSPORT_CLOSED: shared worker app server closed unexpectedly"
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
            state.worker = Some(WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now(),
            });
        }

        tokio::task::yield_now().await;
        manager.prune_worker().await?;
        assert!(manager.state.lock().await.worker.is_none());
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
            state.worker = Some(WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx: events_tx.clone(),
                worker,
                last_active_at: Instant::now(),
            });
        }

        let mut receiver = manager.subscribe("conversation-1").await?;
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
            state.worker = Some(WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now() - IDLE_WORKER_TTL,
            });
        }

        manager.prune_worker_at(Instant::now()).await?;
        assert!(manager.state.lock().await.worker.is_none());
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
            state.worker = Some(WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now(),
            });
            let handle = state.worker.as_ref().expect("worker handle");
            assert!(!should_evict_worker(handle, Instant::now()));
        }

        manager.prune_worker_at(Instant::now()).await?;
        assert!(manager.state.lock().await.worker.is_some());
        Ok(())
    }
}
