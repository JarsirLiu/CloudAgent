use agent_core::RolloutItem;
use anyhow::{Context, Result, anyhow};
use storage::JsonConversationStore;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub(crate) struct RolloutRecorder {
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
    pub(crate) fn new(store: JsonConversationStore) -> Self {
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

    pub(crate) fn record_items(
        &self,
        conversation_id: impl Into<String>,
        items: &[RolloutItem],
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        self.tx
            .send(RolloutCmd::AddItems {
                conversation_id: conversation_id.into(),
                items: items.to_vec(),
            })
            .map_err(|err| anyhow!("failed to queue rollout items: {err}"))
    }

    pub(crate) async fn flush(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(RolloutCmd::Flush { ack: tx })
            .map_err(|err| anyhow!("failed to queue rollout flush: {err}"))?;
        rx.await
            .context("failed waiting for rollout flush")?
            .context("rollout flush failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{EventMsg, ResponseItem};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn record_items_preserves_queue_order() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cloudagent-rollout-recorder-{unique}"));
        let store = JsonConversationStore::new(&root);
        let recorder = RolloutRecorder::new(store.clone());
        let conversation_id = "default";

        recorder
            .record_items(
                conversation_id,
                &[RolloutItem::from(ResponseItem::User {
                    content: "hello".to_string(),
                })],
            )
            .expect("record response item");
        recorder
            .record_items(
                conversation_id,
                &[RolloutItem::from(EventMsg::TurnStarted {
                    turn_id: "turn-1".to_string(),
                    conversation_id: conversation_id.to_string(),
                    user_input: "hello".to_string(),
                })],
            )
            .expect("record event");
        recorder.flush().await.expect("flush rollout");

        let items = store
            .load_rollout_items(conversation_id)
            .await
            .expect("load rollout items");
        assert!(matches!(items[0], RolloutItem::ResponseItem { .. }));
        assert!(matches!(items[1], RolloutItem::EventMsg { .. }));

        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
