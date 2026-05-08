use agent_app_server_client::{AppServerClient, AppServerEvent, StdioClientConfig};
use agent_protocol::AppClientCommand;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::time::{Duration, Instant};

const IDLE_WORKER_TTL: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub(crate) struct WorkerManager {
    worker_program: OsString,
    state: Arc<Mutex<WorkerManagerState>>,
}

impl WorkerManager {
    pub(crate) fn new(worker_program: OsString) -> Self {
        Self {
            worker_program,
            state: Arc::new(Mutex::new(WorkerManagerState::default())),
        }
    }

    pub(crate) async fn subscribe(
        &self,
        conversation_id: &str,
    ) -> Result<broadcast::Receiver<NodeEvent>> {
        self.prune_workers().await?;
        let mut state = self.state.lock().await;
        let handle = self.ensure_worker(&mut state, conversation_id).await?;
        Ok(handle.events_tx.subscribe())
    }

    pub(crate) async fn send_command(
        &self,
        conversation_id: &str,
        command: AppClientCommand,
    ) -> Result<()> {
        self.prune_workers().await?;
        match self
            .send_command_inner(conversation_id, command.clone())
            .await
        {
            Ok(()) => Ok(()),
            Err(error)
                if error
                    .to_string()
                    .contains("worker command channel closed for") =>
            {
                let mut state = self.state.lock().await;
                state.workers.remove(conversation_id);
                drop(state);
                self.send_command_inner(conversation_id, command).await
            }
            Err(error) => Err(error),
        }
    }

    async fn send_command_inner(
        &self,
        conversation_id: &str,
        command: AppClientCommand,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let handle = self.ensure_worker(&mut state, conversation_id).await?;
        handle.last_active_at = Instant::now();
        handle
            .command_tx
            .send(command)
            .map_err(|_| anyhow!("worker command channel closed for {conversation_id}"))
    }

    async fn ensure_worker<'a>(
        &self,
        state: &'a mut WorkerManagerState,
        conversation_id: &str,
    ) -> Result<&'a mut WorkerHandle> {
        if !state.workers.contains_key(conversation_id) {
            let mut client = AppServerClient::stdio(StdioClientConfig {
                program: self.worker_program.clone(),
                args: worker_stdio_args(conversation_id),
            })
            .await
            .with_context(|| format!("failed to start worker for {conversation_id}"))?;
            let (command_tx, mut command_rx) = mpsc::unbounded_channel();
            let (events_tx, _) = broadcast::channel(128);
            let conversation_id_owned = conversation_id.to_string();
            let events_tx_for_worker = events_tx.clone();
            let worker = tokio::spawn(async move {
                loop {
                    tokio::select! {
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
                                    let _ = events_tx_for_worker.send(NodeEvent::Message { message });
                                }
                                Some(AppServerEvent::Lagged { skipped }) => {
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: conversation_id_owned.clone(),
                                        message: format!("worker event channel lagged; skipped {skipped} events"),
                                        is_error: false,
                                    });
                                }
                                Some(AppServerEvent::Disconnected { message }) => {
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: conversation_id_owned.clone(),
                                        message,
                                        is_error: true,
                                    });
                                    return Result::<()>::Ok(());
                                }
                                None => {
                                    let _ = events_tx_for_worker.send(NodeEvent::Diagnostic {
                                        conversation_id: conversation_id_owned.clone(),
                                        message: "worker event stream ended".to_string(),
                                        is_error: true,
                                    });
                                    return Result::<()>::Ok(());
                                }
                            }
                        }
                    }
                }
            });
            state.workers.insert(
                conversation_id.to_string(),
                WorkerHandle {
                    command_tx,
                    events_tx,
                    worker,
                    last_active_at: Instant::now(),
                },
            );
        }
        state
            .workers
            .get_mut(conversation_id)
            .ok_or_else(|| anyhow!("worker handle missing for {conversation_id}"))
    }

    async fn prune_workers(&self) -> Result<()> {
        self.prune_workers_at(Instant::now()).await
    }

    async fn prune_workers_at(&self, now: Instant) -> Result<()> {
        let evicted = {
            let mut state = self.state.lock().await;
            let evicted: Vec<String> = state
                .workers
                .iter()
                .filter_map(|(conversation_id, handle)| {
                    should_evict_worker(handle, now).then(|| conversation_id.to_string())
                })
                .collect();

            evicted
                .into_iter()
                .filter_map(|conversation_id| {
                    state
                        .workers
                        .remove(&conversation_id)
                        .map(|handle| (conversation_id, handle))
                })
                .collect::<Vec<_>>()
        };

        for (_, handle) in evicted {
            drop(handle.command_tx);
            handle.worker.await??;
        }
        Ok(())
    }
}

#[derive(Default)]
struct WorkerManagerState {
    workers: HashMap<String, WorkerHandle>,
}

#[derive(Clone, Debug)]
pub(crate) enum NodeEvent {
    Message {
        message: agent_protocol::AppServerMessage,
    },
    Diagnostic {
        conversation_id: String,
        message: String,
        is_error: bool,
    },
}

struct WorkerHandle {
    command_tx: mpsc::UnboundedSender<AppClientCommand>,
    events_tx: broadcast::Sender<NodeEvent>,
    worker: tokio::task::JoinHandle<Result<()>>,
    last_active_at: Instant,
}

fn should_evict_worker(handle: &WorkerHandle, now: Instant) -> bool {
    handle.worker.is_finished() || now.duration_since(handle.last_active_at) >= IDLE_WORKER_TTL
}

fn worker_stdio_args(conversation_id: &str) -> Vec<OsString> {
    vec![
        OsString::from("app-server-stdio"),
        OsString::from("--conversation"),
        OsString::from(conversation_id),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        IDLE_WORKER_TTL, NodeEvent, WorkerHandle, WorkerManager, should_evict_worker,
        worker_stdio_args,
    };
    use anyhow::Result;
    use std::ffi::OsString;
    use tokio::sync::{broadcast, mpsc};
    use tokio::time::Instant;

    #[test]
    fn builds_worker_stdio_arguments() {
        assert_eq!(
            worker_stdio_args("conversation-42"),
            vec![
                OsString::from("app-server-stdio"),
                OsString::from("--conversation"),
                OsString::from("conversation-42"),
            ]
        );
    }

    #[tokio::test]
    async fn prune_finished_workers_removes_completed_handles() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"));
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let worker = tokio::spawn(async { Result::<()>::Ok(()) });
        {
            let mut state = manager.state.lock().await;
            let (events_tx, _) = broadcast::channel(8);
            state.workers.insert(
                "conversation-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
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
    async fn subscribe_receives_broadcast_events_from_existing_worker() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"));
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
                "conversation-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
                    events_tx: events_tx.clone(),
                    worker,
                    last_active_at: Instant::now(),
                },
            );
        }

        let mut receiver = manager.subscribe("conversation-1").await?;
        let _ = events_tx.send(NodeEvent::Diagnostic {
            conversation_id: "conversation-1".to_string(),
            message: "hello".to_string(),
            is_error: false,
        });

        match receiver.recv().await? {
            NodeEvent::Diagnostic {
                conversation_id,
                message,
                is_error,
            } => {
                assert_eq!(conversation_id, "conversation-1");
                assert_eq!(message, "hello");
                assert!(!is_error);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn prune_workers_evicts_idle_handles() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"));
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
                "conversation-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
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
    async fn prune_workers_keeps_recent_handles() -> Result<()> {
        let manager = WorkerManager::new(OsString::from("agentd.exe"));
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
                "conversation-1".to_string(),
                WorkerHandle {
                    command_tx: tx,
                    events_tx,
                    worker,
                    last_active_at: Instant::now(),
                },
            );
            let handle = state.workers.get("conversation-1").expect("worker handle");
            assert!(!should_evict_worker(handle, Instant::now()));
        }

        manager.prune_workers_at(Instant::now()).await?;
        assert!(
            manager
                .state
                .lock()
                .await
                .workers
                .contains_key("conversation-1")
        );
        Ok(())
    }
}
