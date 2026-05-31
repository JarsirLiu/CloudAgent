use agent_core::{
    RolloutItem, RolloutPersistenceMode, RolloutRecorderBackend, persisted_rollout_items,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::JsonConversationStore;

#[derive(Clone)]
pub struct RolloutRecorder {
    tx: mpsc::UnboundedSender<RolloutCmd>,
}

enum RolloutCmd {
    AddItems {
        conversation_id: String,
        items: Vec<RolloutItem>,
    },
    Flush {
        ack: oneshot::Sender<Result<()>>,
    },
}

impl RolloutRecorder {
    pub fn new(store: JsonConversationStore) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<RolloutCmd>();
        tokio::spawn(async move {
            let mut last_error: Option<String> = None;
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    RolloutCmd::AddItems {
                        conversation_id,
                        items,
                    } => {
                        if let Err(err) = store.append_rollout_items(&conversation_id, &items).await
                        {
                            tracing::error!("failed to record rollout items: {err:#}");
                            last_error = Some(format!("{err:#}"));
                        }
                    }
                    RolloutCmd::Flush { ack } => {
                        let result = match last_error.take() {
                            Some(err) => Err(anyhow!("failed to record rollout items: {err}")),
                            None => Ok(()),
                        };
                        let _ = ack.send(result);
                    }
                }
            }
        });
        Self { tx }
    }
}

#[async_trait]
impl RolloutRecorderBackend for RolloutRecorder {
    fn record_items(&self, conversation_id: &str, items: &[RolloutItem]) -> Result<()> {
        let items = persisted_rollout_items(items, RolloutPersistenceMode::Limited);
        if items.is_empty() {
            return Ok(());
        }
        self.tx
            .send(RolloutCmd::AddItems {
                conversation_id: conversation_id.to_string(),
                items,
            })
            .map_err(|err| anyhow!("failed to queue rollout items: {err}"))
    }

    async fn flush(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(RolloutCmd::Flush { ack: tx })
            .map_err(|err| anyhow!("failed to queue rollout flush: {err}"))?;
        rx.await
            .context("failed waiting for rollout flush")?
            .context("rollout flush failed")
    }
}
