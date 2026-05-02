use crate::AgentRuntime;
use agent_core::{
    ConversationHistory, ConversationState, ConversationTurn, build_turns_from_rollout_items,
    flatten_conversation_turns,
};
use agent_protocol::{ConversationSnapshot, ConversationStatus, TranscriptItem};
use anyhow::Result;

impl AgentRuntime {
    pub async fn conversation_history_snapshot(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationHistory> {
        Ok(self
            .conversation_snapshot(conversation_id)
            .await?
            .history()
            .clone())
    }

    pub async fn conversation_transcript_snapshot(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<TranscriptItem>> {
        Ok(flatten_conversation_turns(
            &self.build_turns_from_rollout(conversation_id).await?,
        ))
    }

    pub async fn build_turns_from_rollout(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<ConversationTurn>> {
        self.rollout_recorder.flush().await?;
        let rollout_items = self.store.load_rollout_items(conversation_id).await?;
        Ok(build_turns_from_rollout_items(&rollout_items))
    }

    pub async fn conversation_snapshot(&self, conversation_id: &str) -> Result<ConversationState> {
        if let Some(conversation) = self.state.conversation(conversation_id).await {
            return Ok(conversation);
        }
        if let Some(mut conversation) = self.store.load_conversation(conversation_id).await? {
            conversation
                .context_mut()
                .ensure_system_prompt(self.config.runtime.system_prompt.clone());
            return Ok(conversation);
        }
        Ok(ConversationState::new(ConversationHistory::new(
            conversation_id.to_string(),
            self.config.runtime.system_prompt.clone(),
        )))
    }

    pub async fn conversation_status(&self, conversation_id: &str) -> Result<ConversationSnapshot> {
        let history = self.conversation_history_snapshot(conversation_id).await?;
        let active_turn = self.state.active_turn(conversation_id).await;
        Ok(ConversationSnapshot {
            conversation_id: conversation_id.to_string(),
            conversation_status: if active_turn.is_some() {
                ConversationStatus::Busy
            } else {
                ConversationStatus::Idle
            },
            active_turn: active_turn.as_ref().map(|turn| turn.turn_id.clone()),
            turn_state: active_turn.as_ref().map(|turn| turn.turn_state.clone()),
            message_count: crate::engine::visible_message_count(&history),
        })
    }
}
