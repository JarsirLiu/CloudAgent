use crate::AgentRuntime;
use agent_core::{ConversationHistory, RolloutItem, conversation_history_from_rollout_items};
use anyhow::Result;

impl AgentRuntime {
    pub(crate) async fn load_history(&self, conversation_id: &str) -> Result<ConversationHistory> {
        if let Some(history) = self.state.history(conversation_id).await
            && !is_placeholder_history(&history)
        {
            return Ok(history);
        }

        self.rollout_recorder.flush().await?;
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        if !rollout_items.is_empty() {
            let history = conversation_history_from_rollout_items(
                conversation_id.to_string(),
                self.config.runtime.system_prompt.clone(),
                &rollout_items,
            );
            self.save_history(history.clone()).await?;
            return Ok(history);
        }

        let history = ConversationHistory::new(
            conversation_id.to_string(),
            self.config.runtime.system_prompt.clone(),
        );
        self.save_history(history.clone()).await?;
        Ok(history)
    }

    pub(crate) async fn history_from_rollout(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationHistory> {
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        Ok(conversation_history_from_rollout_items(
            conversation_id.to_string(),
            self.config.runtime.system_prompt.clone(),
            &rollout_items,
        ))
    }

    pub(crate) async fn save_history(&self, history: ConversationHistory) -> Result<()> {
        self.state.save_history(history).await;
        Ok(())
    }

    pub(crate) async fn persist_rollout_items(
        &self,
        conversation_id: &str,
        items: &[RolloutItem],
    ) -> Result<()> {
        self.record_rollout_items(conversation_id, items)
    }

    pub(crate) fn record_rollout_items(
        &self,
        conversation_id: &str,
        items: &[RolloutItem],
    ) -> Result<()> {
        self.rollout_recorder.record_items(conversation_id, items)
    }
}

fn is_placeholder_history(history: &ConversationHistory) -> bool {
    history.turn_count == 0
        && matches!(
            history.messages.as_slice(),
            [agent_core::ResponseItem::System { .. }]
        )
}
