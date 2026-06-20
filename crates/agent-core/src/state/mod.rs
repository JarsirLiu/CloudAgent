use crate::{ActiveConversationTurn, ConversationHistory, ConversationState};
use crate::{EventMsg, RequestId, ServerRequest, TurnState};
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ActiveTurnHandle {
    pub turn_id: String,
    pub turn_state: TurnState,
    pub cancellation_token: CancellationToken,
}

impl ActiveTurnHandle {
    fn from_parts(
        active_turn: &ActiveConversationTurn,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            turn_id: active_turn.turn_id.clone(),
            turn_state: active_turn.turn_state.clone(),
            cancellation_token,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }
}

struct AgentConversationEntry {
    conversation: ConversationState,
    cancellation_token: Option<CancellationToken>,
}

impl AgentConversationEntry {
    fn new(history: ConversationHistory) -> Self {
        Self {
            conversation: ConversationState::new(history),
            cancellation_token: None,
        }
    }
}

pub struct AgentState {
    system_prompt: String,
    conversations: StdMutex<HashMap<String, AgentConversationEntry>>,
}

impl AgentState {
    pub fn new(system_prompt: String) -> Self {
        Self {
            system_prompt,
            conversations: StdMutex::new(HashMap::new()),
        }
    }

    pub async fn conversation(&self, conversation_id: &str) -> Option<ConversationState> {
        self.conversations
            .lock()
            .ok()?
            .get(conversation_id)
            .map(|entry| entry.conversation.clone())
    }

    pub async fn history(&self, conversation_id: &str) -> Option<ConversationHistory> {
        self.conversations
            .lock()
            .ok()?
            .get(conversation_id)
            .map(|entry| entry.conversation.history().clone())
    }

    pub async fn save_history(&self, history: ConversationHistory) {
        let Ok(mut conversations) = self.conversations.lock() else {
            return;
        };
        let conversation_id = history.id.clone();
        if let Some(entry) = conversations.get_mut(&conversation_id) {
            *entry.conversation.history_mut() = history;
        } else {
            conversations.insert(conversation_id, AgentConversationEntry::new(history));
        }
    }

    pub async fn remove_conversation(&self, conversation_id: &str) {
        if let Ok(mut conversations) = self.conversations.lock() {
            conversations.remove(conversation_id);
        }
    }

    pub async fn active_turn(&self, conversation_id: &str) -> Option<ActiveTurnHandle> {
        let conversations = self.conversations.lock().ok()?;
        let entry = conversations.get(conversation_id)?;
        let active_turn = entry.conversation.active_turn.as_ref()?;
        let cancellation_token = entry.cancellation_token.clone()?;
        Some(ActiveTurnHandle::from_parts(
            active_turn,
            cancellation_token,
        ))
    }

    pub async fn start_turn(
        &self,
        conversation_id: String,
        turn_id: String,
    ) -> Option<ActiveTurnHandle> {
        let cancellation_token = CancellationToken::new();
        let active_turn = ActiveTurnHandle {
            turn_id: turn_id.clone(),
            turn_state: TurnState::Running,
            cancellation_token: cancellation_token.clone(),
        };

        let Ok(mut conversations) = self.conversations.lock() else {
            return Some(active_turn);
        };
        let entry = conversations
            .entry(conversation_id.clone())
            .or_insert_with(|| {
                AgentConversationEntry::new(ConversationHistory::new(
                    conversation_id,
                    self.system_prompt.clone(),
                ))
            });
        if entry.conversation.active_turn.is_some() {
            return None;
        }
        entry.conversation.set_active_turn(turn_id);
        entry.cancellation_token = Some(cancellation_token);

        Some(active_turn)
    }

    pub async fn finish_turn(&self, conversation_id: &str) {
        if let Ok(mut conversations) = self.conversations.lock()
            && let Some(entry) = conversations.get_mut(conversation_id)
        {
            entry.conversation.clear_active_turn();
            entry.cancellation_token = None;
        }
    }

    pub async fn update_turn_state(
        &self,
        conversation_id: &str,
        turn_id: &str,
        turn_state: TurnState,
    ) {
        if let Ok(mut conversations) = self.conversations.lock()
            && let Some(entry) = conversations.get_mut(conversation_id)
        {
            entry.conversation.update_turn_state(turn_id, turn_state);
        }
    }

    pub async fn interrupt_conversation(&self, conversation_id: &str) -> bool {
        let Ok(mut conversations) = self.conversations.lock() else {
            return false;
        };
        let Some(entry) = conversations.get_mut(conversation_id) else {
            return false;
        };
        let Some(cancellation_token) = entry.cancellation_token.clone() else {
            return false;
        };
        cancellation_token.cancel();
        if let Some(active_turn) = entry.conversation.active_turn.as_mut() {
            active_turn.turn_state = TurnState::Cancelled;
        }
        true
    }

    pub fn append_conversation_event(&self, conversation_id: &str, event: EventMsg) {
        let Ok(mut conversations) = self.conversations.lock() else {
            return;
        };
        let entry = conversations
            .entry(conversation_id.to_string())
            .or_insert_with(|| {
                AgentConversationEntry::new(ConversationHistory::new(
                    conversation_id.to_string(),
                    self.system_prompt.clone(),
                ))
            });
        entry.conversation.append_event(event);
    }

    pub async fn set_pending_request(
        &self,
        conversation_id: &str,
        request_id: RequestId,
        request: ServerRequest,
    ) {
        let Ok(mut conversations) = self.conversations.lock() else {
            return;
        };
        let entry = conversations
            .entry(conversation_id.to_string())
            .or_insert_with(|| {
                AgentConversationEntry::new(ConversationHistory::new(
                    conversation_id.to_string(),
                    self.system_prompt.clone(),
                ))
            });
        entry.conversation.set_pending_request(request_id, request);
    }

    pub async fn resolve_pending_request(&self, conversation_id: &str, request_id: &RequestId) {
        if let Ok(mut conversations) = self.conversations.lock()
            && let Some(entry) = conversations.get_mut(conversation_id)
        {
            entry.conversation.resolve_pending_request(request_id);
        }
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
