use crate::AgentRuntime;
use agent_core::{
    ConversationHistory, ConversationState, RolloutItem, conversation_history_from_rollout_items,
};
use anyhow::Result;

impl AgentRuntime {
    pub(crate) async fn load_history(&self, conversation_id: &str) -> Result<ConversationHistory> {
        if let Some(history) = self.state.history(conversation_id).await {
            return Ok(history);
        }

        self.rollout_recorder.flush().await?;
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        if rollout_items
            .iter()
            .any(|item| matches!(item, RolloutItem::ResponseItem { .. }))
        {
            let history = conversation_history_from_rollout_items(
                conversation_id.to_string(),
                self.config.runtime.system_prompt.clone(),
                &rollout_items,
            );
            self.save_history(history.clone()).await?;
            return Ok(history);
        }

        let mut conversation =
            if let Some(conversation) = self.store.load_conversation(conversation_id).await? {
                conversation
            } else {
                ConversationState::new(ConversationHistory::new(
                    conversation_id.to_string(),
                    self.config.runtime.system_prompt.clone(),
                ))
            };
        conversation
            .context_mut()
            .ensure_system_prompt(self.config.runtime.system_prompt.clone());
        let history = conversation.history().clone();
        self.state.save_conversation(conversation).await;
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
        let conversation_id = history.id.clone();
        self.state.save_history(history).await;
        if let Some(conversation) = self.state.conversation(&conversation_id).await {
            self.store.save_conversation(&conversation).await?;
        }
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
