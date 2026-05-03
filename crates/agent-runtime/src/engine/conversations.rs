use crate::AgentRuntime;
use agent_core::{
    ConversationHistory, ConversationState, ConversationTurn, ModelRequest, ResponseItem,
    build_turns_from_rollout_items,
    flatten_conversation_turns,
};
use agent_protocol::{ConversationSnapshot, ConversationStatus, ConversationSummary, TranscriptItem};
use anyhow::Result;
use uuid::Uuid;

fn paginate_turns(
    turns: Vec<ConversationTurn>,
    before_turn_id: Option<&str>,
    limit: usize,
) -> (Vec<ConversationTurn>, bool, Option<String>) {
    if turns.is_empty() {
        return (Vec::new(), false, None);
    }
    let end_exclusive = if let Some(before_id) = before_turn_id {
        turns
            .iter()
            .position(|turn| turn.id == before_id)
            .unwrap_or(turns.len())
    } else {
        turns.len()
    };
    let page_limit = limit.max(1);
    let start = end_exclusive.saturating_sub(page_limit);
    let page = turns[start..end_exclusive].to_vec();
    let has_more = start > 0;
    let next_before_turn_id = if has_more {
        Some(turns[start].id.clone())
    } else {
        None
    };
    (page, has_more, next_before_turn_id)
}

impl AgentRuntime {
    pub async fn ensure_active_conversation(&self) -> Result<String> {
        if let Some(id) = self.store.load_active_conversation().await?
            && !id.trim().is_empty()
        {
            return Ok(id);
        }
        let id = Uuid::now_v7().to_string();
        self.store.create_conversation(&id).await?;
        self.store.mark_active_conversation(&id).await?;
        Ok(id)
    }

    pub async fn mark_active_conversation(&self, conversation_id: &str) -> Result<()> {
        self.store.mark_active_conversation(conversation_id).await
    }

    pub async fn load_active_conversation(&self) -> Result<Option<String>> {
        self.store.load_active_conversation().await
    }

    pub async fn reset_conversation(&self, conversation_id: &str) -> Result<()> {
        self.rollout_recorder.flush().await?;
        self.state.remove_conversation(conversation_id).await;
        self.store.delete_conversation(conversation_id).await?;
        self.store.delete_events(conversation_id).await
    }

    pub async fn create_conversation(&self, conversation_id: &str) -> Result<()> {
        self.store.create_conversation(conversation_id).await
    }

    pub async fn archive_conversation(&self, conversation_id: &str) -> Result<()> {
        self.rollout_recorder.flush().await?;
        self.state.remove_conversation(conversation_id).await;
        self.store.archive_conversation(conversation_id).await
    }

    pub async fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        Ok(self
            .store
            .list_conversations()
            .await?
            .into_iter()
            .map(|summary| ConversationSummary {
                conversation_id: summary.conversation_id,
                title: summary.title,
                message_count: summary.message_count,
                updated_at_ms: summary.updated_at_ms,
            })
            .collect())
    }

    pub async fn set_conversation_title(&self, conversation_id: &str, title: &str) -> Result<()> {
        self.store.set_conversation_title(conversation_id, title).await
    }

    pub async fn suggest_conversation_title(&self, user_input: &str) -> Result<String> {
        let request = ModelRequest {
            messages: vec![
                ResponseItem::System {
                    content: "Generate a short session title (max 8 words). Return title text only."
                        .to_string(),
                },
                ResponseItem::User {
                    content: user_input.to_string(),
                },
            ],
            tools: Vec::new(),
            temperature: 0.2,
        };
        let response = self.model.complete(request).await?;
        let title = response
            .content
            .unwrap_or_default()
            .trim()
            .trim_matches('"')
            .to_string();
        Ok(title)
    }

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

    pub async fn build_turns_page_from_rollout(
        &self,
        conversation_id: &str,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<ConversationTurn>, bool, Option<String>)> {
        let turns = self.build_turns_from_rollout(conversation_id).await?;
        Ok(paginate_turns(turns, before_turn_id, limit))
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
            message_count: super::visible_message_count(&history),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::paginate_turns;
    use agent_core::ConversationTurn;
    use agent_protocol::TurnState;

    fn turn(id: &str) -> ConversationTurn {
        ConversationTurn {
            id: id.to_string(),
            state: TurnState::Completed,
            items: Vec::new(),
            rollout_start_index: 0,
            rollout_end_index: 0,
        }
    }

    #[test]
    fn paginate_returns_tail_page_and_cursor() {
        let turns = vec![turn("t1"), turn("t2"), turn("t3"), turn("t4")];
        let (page, has_more, cursor) = paginate_turns(turns, None, 2);
        assert_eq!(page.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t3", "t4"]);
        assert!(has_more);
        assert_eq!(cursor.as_deref(), Some("t3"));
    }

    #[test]
    fn paginate_before_cursor_returns_older_page() {
        let turns = vec![turn("t1"), turn("t2"), turn("t3"), turn("t4")];
        let (page, has_more, cursor) = paginate_turns(turns, Some("t3"), 2);
        assert_eq!(page.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["t1", "t2"]);
        assert!(!has_more);
        assert!(cursor.is_none());
    }
}
