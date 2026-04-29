use crate::conversation::ConversationHistory;
use agent_protocol::{RequestId, ServerRequest, TurnEvent, TurnState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActiveConversationTurn {
    pub turn_id: String,
    pub turn_state: TurnState,
}

impl ActiveConversationTurn {
    pub fn new(turn_id: impl Into<String>) -> Self {
        Self {
            turn_id: turn_id.into(),
            turn_state: TurnState::Running,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingConversationRequest {
    pub request_id: RequestId,
    pub request: ServerRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationState {
    pub history: ConversationHistory,
    pub active_turn: Option<ActiveConversationTurn>,
    pub event_log: Vec<TurnEvent>,
    pub pending_requests: Vec<PendingConversationRequest>,
}

impl ConversationState {
    pub fn new(history: ConversationHistory) -> Self {
        Self {
            history,
            active_turn: None,
            event_log: Vec::new(),
            pending_requests: Vec::new(),
        }
    }

    pub fn set_active_turn(&mut self, turn_id: impl Into<String>) {
        self.active_turn = Some(ActiveConversationTurn::new(turn_id));
    }

    pub fn clear_active_turn(&mut self) {
        self.active_turn = None;
    }

    pub fn update_turn_state(&mut self, turn_id: &str, turn_state: TurnState) {
        if let Some(active_turn) = self.active_turn.as_mut()
            && active_turn.turn_id == turn_id
        {
            active_turn.turn_state = turn_state;
        }
    }

    pub fn append_event(&mut self, event: TurnEvent) {
        self.event_log.push(event);
    }

    pub fn set_pending_request(&mut self, request_id: RequestId, request: ServerRequest) {
        self.pending_requests.push(PendingConversationRequest {
            request_id,
            request,
        });
    }

    pub fn resolve_pending_request(&mut self, request_id: &RequestId) {
        self.pending_requests
            .retain(|pending| &pending.request_id != request_id);
    }

    pub fn history(&self) -> &ConversationHistory {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut ConversationHistory {
        &mut self.history
    }

    pub fn pending_requests(&self) -> &[PendingConversationRequest] {
        &self.pending_requests
    }

    pub fn persisted_record(&self) -> PersistedConversation {
        PersistedConversation {
            history: self.history.clone(),
            pending_requests: self.pending_requests.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedConversation {
    pub history: ConversationHistory,
    #[serde(default)]
    pub pending_requests: Vec<PendingConversationRequest>,
}

impl PersistedConversation {
    pub fn into_state(self) -> ConversationState {
        ConversationState {
            history: self.history,
            active_turn: None,
            event_log: Vec::new(),
            pending_requests: self.pending_requests,
        }
    }
}
