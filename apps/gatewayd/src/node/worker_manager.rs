use agent_app_server_client::{AppServerClient, AppServerEvent, StdioClientConfig};
use agent_protocol::AppClientCommand;
use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::ffi::OsString;
use tokio::sync::mpsc;
use tokio::time::Instant;

pub(crate) struct WorkerManager {
    worker_program: OsString,
    workers: HashMap<String, WorkerHandle>,
}

impl WorkerManager {
    pub(crate) fn new(worker_program: OsString) -> Self {
        Self {
            worker_program,
            workers: HashMap::new(),
        }
    }

    pub(crate) async fn send_command(
        &mut self,
        conversation_id: &str,
        command: AppClientCommand,
        event_tx: mpsc::Sender<NodeEvent>,
    ) -> Result<()> {
        self.prune_finished_workers().await?;
        match self
            .send_command_inner(conversation_id, command.clone(), event_tx.clone())
            .await
        {
            Ok(()) => Ok(()),
            Err(error)
                if error
                    .to_string()
                    .contains("worker command channel closed for") =>
            {
                self.workers.remove(conversation_id);
                self.send_command_inner(conversation_id, command, event_tx)
                    .await
            }
            Err(error) => Err(error),
        }
    }

    async fn send_command_inner(
        &mut self,
        conversation_id: &str,
        command: AppClientCommand,
        event_tx: mpsc::Sender<NodeEvent>,
    ) -> Result<()> {
        let handle = self.ensure_worker(conversation_id, event_tx).await?;
        handle.last_active_at = Instant::now();
        handle
            .command_tx
            .send(command)
            .map_err(|_| anyhow!("worker command channel closed for {conversation_id}"))
    }

    async fn ensure_worker(
        &mut self,
        conversation_id: &str,
        event_tx: mpsc::Sender<NodeEvent>,
    ) -> Result<&mut WorkerHandle> {
        if !self.workers.contains_key(conversation_id) {
            let mut client = AppServerClient::stdio(StdioClientConfig {
                program: self.worker_program.clone(),
                args: worker_stdio_args(conversation_id),
            })
            .await
            .with_context(|| format!("failed to start worker for {conversation_id}"))?;
            let (command_tx, mut command_rx) = mpsc::unbounded_channel();
            let conversation_id_owned = conversation_id.to_string();
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
                                    if event_tx
                                        .send(NodeEvent::Message { message })
                                        .await
                                        .is_err()
                                    {
                                        client.shutdown().await?;
                                        return Result::<()>::Ok(());
                                    }
                                }
                                Some(AppServerEvent::Lagged { skipped }) => {
                                    if event_tx
                                        .send(NodeEvent::Diagnostic {
                                            conversation_id: conversation_id_owned.clone(),
                                            message: format!("worker event channel lagged; skipped {skipped} events"),
                                            is_error: false,
                                        })
                                        .await
                                        .is_err()
                                    {
                                        client.shutdown().await?;
                                        return Result::<()>::Ok(());
                                    }
                                }
                                Some(AppServerEvent::Disconnected { message }) => {
                                    let _ = event_tx.send(NodeEvent::Diagnostic {
                                        conversation_id: conversation_id_owned.clone(),
                                        message,
                                        is_error: true,
                                    }).await;
                                    return Result::<()>::Ok(());
                                }
                                None => {
                                    let _ = event_tx.send(NodeEvent::Diagnostic {
                                        conversation_id: conversation_id_owned.clone(),
                                        message: "worker event stream ended".to_string(),
                                        is_error: true,
                                    }).await;
                                    return Result::<()>::Ok(());
                                }
                            }
                        }
                    }
                }
            });
            self.workers.insert(
                conversation_id.to_string(),
                WorkerHandle {
                    command_tx,
                    worker,
                    last_active_at: Instant::now(),
                },
            );
        }
        self.workers
            .get_mut(conversation_id)
            .ok_or_else(|| anyhow!("worker handle missing for {conversation_id}"))
    }

    async fn prune_finished_workers(&mut self) -> Result<()> {
        let finished: Vec<String> = self
            .workers
            .iter()
            .filter_map(|(conversation_id, handle)| {
                handle
                    .worker
                    .is_finished()
                    .then(|| conversation_id.to_string())
            })
            .collect();

        for conversation_id in finished {
            if let Some(handle) = self.workers.remove(&conversation_id) {
                handle.worker.await??;
            }
        }
        Ok(())
    }

    pub(crate) async fn shutdown(self) -> Result<()> {
        for (_, handle) in self.workers {
            drop(handle.command_tx);
            handle.worker.await??;
        }
        Ok(())
    }
}

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
    worker: tokio::task::JoinHandle<Result<()>>,
    last_active_at: Instant,
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
    use super::{WorkerHandle, WorkerManager, worker_stdio_args};
    use anyhow::Result;
    use std::ffi::OsString;
    use tokio::sync::mpsc;
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
        let mut manager = WorkerManager::new(OsString::from("agentd.exe"));
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let worker = tokio::spawn(async { Result::<()>::Ok(()) });
        manager.workers.insert(
            "conversation-1".to_string(),
            WorkerHandle {
                command_tx: tx,
                worker,
                last_active_at: Instant::now(),
            },
        );

        tokio::task::yield_now().await;
        manager.prune_finished_workers().await?;
        assert!(manager.workers.is_empty());
        Ok(())
    }
}
