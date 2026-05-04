use crate::context::ContextManager;
use crate::conversation::ConversationHistory;
use crate::turn::{EventMsg, RequestId, ServerRequest, TurnState};
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
    pub context: ContextManager,
    pub active_turn: Option<ActiveConversationTurn>,
    pub event_log: Vec<EventMsg>,
    pub pending_requests: Vec<PendingConversationRequest>,
}

impl ConversationState {
    pub fn new(history: ConversationHistory) -> Self {
        Self {
            context: ContextManager::from_history(history),
            active_turn: None,
            event_log: Vec::new(),
            pending_requests: Vec::new(),
        }
    }

    pub fn set_active_turn(&mut self, turn_id: impl Into<String>) {
        self.active_turn = Some(ActiveConversationTurn::new(turn_id));
    }

    pub fn clear_active_turn(&mut self) {
        if let Some(active_turn) = &self.active_turn {
            let turn_id = active_turn.turn_id.clone();
            self.pending_requests
                .retain(|pending| match &pending.request {
                    ServerRequest::ToolApproval { request } => request.turn_id != turn_id,
                });
        }
        self.active_turn = None;
    }

    pub fn update_turn_state(&mut self, turn_id: &str, turn_state: TurnState) {
        if let Some(active_turn) = self.active_turn.as_mut()
            && active_turn.turn_id == turn_id
        {
            active_turn.turn_state = turn_state;
        }
    }

    pub fn append_event(&mut self, event: EventMsg) {
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
        self.context.history()
    }

    pub fn history_mut(&mut self) -> &mut ConversationHistory {
        self.context.history_mut()
    }

    pub fn context(&self) -> &ContextManager {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut ContextManager {
        &mut self.context
    }

    pub fn pending_requests(&self) -> &[PendingConversationRequest] {
        &self.pending_requests
    }
}
