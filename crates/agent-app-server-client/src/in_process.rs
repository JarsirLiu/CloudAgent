use crate::{AppServerEvent, DEFAULT_EVENT_CHANNEL_CAPACITY, TypedRequestError, forward_event};
use agent_app_server::{InProcessClientHandle, InProcessClientSender, start_in_process};
use agent_core::AgentHost;
use agent_protocol::{AppClientCommand, JsonRpcRequest};
use anyhow::Result;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::{io, io::ErrorKind};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct InProcessClientConfig {
    pub runtime: Arc<AgentHost>,
    pub conversation_id: String,
    pub auto_approve: bool,
    pub auto_approve_reason: Option<String>,
}

pub struct InProcessAppServerClient {
    sender: InProcessClientSender,
    event_rx: mpsc::Receiver<AppServerEvent>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    worker: JoinHandle<Result<()>>,
}

#[derive(Clone)]
pub struct InProcessAppServerRequestHandle {
    sender: InProcessClientSender,
}

impl InProcessAppServerClient {
    pub fn start(config: InProcessClientConfig) -> Self {
        let handle = start_in_process(
            config.runtime,
            config.conversation_id,
            config.auto_approve,
            config.auto_approve_reason,
        );
        let sender = handle.sender();
        let (event_tx, event_rx) = mpsc::channel(DEFAULT_EVENT_CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let worker = tokio::spawn(run_event_worker(handle, event_tx, shutdown_rx));

        Self {
            sender,
            event_rx,
            shutdown_tx: Some(shutdown_tx),
            worker,
        }
    }

    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.sender.send_command(command)
    }

    pub fn request_handle(&self) -> InProcessAppServerRequestHandle {
        InProcessAppServerRequestHandle {
            sender: self.sender.clone(),
        }
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        self.request_handle().request_typed(request).await
    }

    pub async fn next_event(&mut self) -> Option<AppServerEvent> {
        self.event_rx.recv().await
    }

    pub fn try_next_event(&mut self) -> Option<AppServerEvent> {
        self.event_rx.try_recv().ok()
    }

    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        self.worker.await??;
        Ok(())
    }
}

impl InProcessAppServerRequestHandle {
    pub fn send_command(&self, command: AppClientCommand) -> Result<()> {
        self.sender.send_command(command)
    }

    pub async fn request_typed<T>(&self, request: JsonRpcRequest) -> Result<T, TypedRequestError>
    where
        T: DeserializeOwned,
    {
        Err(TypedRequestError::Transport {
            method: request.method,
            source: io::Error::new(
                ErrorKind::Unsupported,
                "typed requests are not implemented for the in-process client yet",
            ),
        })
    }
}

async fn run_event_worker(
    mut handle: InProcessClientHandle,
    event_tx: mpsc::Sender<AppServerEvent>,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> Result<()> {
    let mut skipped_events = 0usize;
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                let _ = handle.shutdown().await;
                break;
            }
            message = handle.next_message() => {
                match message {
                    Some(message) => {
                        if !forward_event(&event_tx, &mut skipped_events, AppServerEvent::Message(message)).await {
                            break;
                        }
                    }
                    None => {
                        let _ = forward_event(
                            &event_tx,
                            &mut skipped_events,
                            AppServerEvent::Disconnected {
                                message: "in-process app server closed".to_string(),
                            },
                        )
                        .await;
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
