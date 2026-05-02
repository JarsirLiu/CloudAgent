use crate::AgentRuntime;
use agent_core::{
    ConversationHistory, ConversationState, ConversationTurn, build_turns_from_rollout_items,
    flatten_conversation_turns,
};
use agent_protocol::{SessionSnapshot, SessionStatus, SessionSummary, TranscriptItem};
use anyhow::Result;

impl AgentRuntime {
    pub async fn reset_conversation(&self, session_id: &str) -> Result<()> {
        self.rollout_recorder.flush().await?;
        self.state.remove_conversation(session_id).await;
        self.store.delete_conversation(session_id).await?;
        self.store.delete_events(session_id).await
    }

    pub async fn create_conversation(&self, session_id: &str) -> Result<()> {
        self.store.create_conversation(session_id).await
    }

    pub async fn archive_conversation(&self, session_id: &str) -> Result<()> {
        self.rollout_recorder.flush().await?;
        self.state.remove_conversation(session_id).await;
        self.store.archive_conversation(session_id).await
    }

    pub async fn list_conversations(&self) -> Result<Vec<SessionSummary>> {
        Ok(self
            .store
            .list_conversations()
            .await?
            .into_iter()
            .map(|summary| SessionSummary {
                session_id: summary.conversation_id,
                message_count: summary.message_count,
                updated_at_ms: summary.updated_at_ms,
            })
            .collect())
    }

    pub async fn conversation_history_snapshot(
        &self,
        session_id: &str,
    ) -> Result<ConversationHistory> {
        Ok(self
            .conversation_snapshot(session_id)
            .await?
            .history()
            .clone())
    }

    pub async fn conversation_transcript_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Vec<TranscriptItem>> {
        Ok(flatten_conversation_turns(
            &self.build_turns_from_rollout(session_id).await?,
        ))
    }

    pub async fn build_turns_from_rollout(
        &self,
        session_id: &str,
    ) -> Result<Vec<ConversationTurn>> {
        self.rollout_recorder.flush().await?;
        let rollout_items = self.store.load_rollout_items(session_id).await?;
        Ok(build_turns_from_rollout_items(&rollout_items))
    }

    pub async fn conversation_snapshot(&self, session_id: &str) -> Result<ConversationState> {
        if let Some(conversation) = self.state.conversation(session_id).await {
            return Ok(conversation);
        }
        if let Some(mut conversation) = self.store.load_conversation(session_id).await? {
            conversation
                .context_mut()
                .ensure_system_prompt(self.config.runtime.system_prompt.clone());
            return Ok(conversation);
        }
        Ok(ConversationState::new(ConversationHistory::new(
            session_id.to_string(),
            self.config.runtime.system_prompt.clone(),
        )))
    }

    pub async fn conversation_status(&self, session_id: &str) -> Result<SessionSnapshot> {
        let history = self.conversation_history_snapshot(session_id).await?;
        let active_turn = self.state.active_turn(session_id).await;
        Ok(SessionSnapshot {
            session_id: session_id.to_string(),
            conversation_status: if active_turn.is_some() {
                SessionStatus::Busy
            } else {
                SessionStatus::Idle
            },
            active_turn: active_turn.as_ref().map(|turn| turn.turn_id.clone()),
            turn_state: active_turn.as_ref().map(|turn| turn.turn_state.clone()),
            message_count: super::visible_message_count(&history),
        })
    }
}
