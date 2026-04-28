use agent_core::AgentSession;
use agent_protocol::TurnState;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct ActiveTurnHandle {
    pub(crate) turn_id: String,
    pub(crate) turn_state: TurnState,
    pub(crate) cancellation_token: CancellationToken,
}

impl ActiveTurnHandle {
    pub(crate) fn new(turn_id: String) -> Self {
        Self {
            turn_id,
            turn_state: TurnState::Running,
            cancellation_token: CancellationToken::new(),
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    pub(crate) fn request_cancel(&self) {
        self.cancellation_token.cancel();
    }
}

pub(crate) struct RuntimeState {
    sessions: Mutex<HashMap<String, AgentSession>>,
    active_turns: Mutex<HashMap<String, ActiveTurnHandle>>,
}

impl RuntimeState {
    pub(crate) fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            active_turns: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn session(&self, session_id: &str) -> Option<AgentSession> {
        self.sessions.lock().await.get(session_id).cloned()
    }

    pub(crate) async fn save_session(&self, session: AgentSession) {
        self.sessions
            .lock()
            .await
            .insert(session.id.clone(), session);
    }

    pub(crate) async fn remove_session(&self, session_id: &str) {
        self.sessions.lock().await.remove(session_id);
        self.active_turns.lock().await.remove(session_id);
    }

    pub(crate) async fn active_turn(&self, session_id: &str) -> Option<ActiveTurnHandle> {
        self.active_turns.lock().await.get(session_id).cloned()
    }

    pub(crate) async fn start_turn(&self, session_id: String, turn_id: String) -> ActiveTurnHandle {
        let handle = ActiveTurnHandle::new(turn_id);
        self.active_turns
            .lock()
            .await
            .insert(session_id, handle.clone());
        handle
    }

    pub(crate) async fn finish_turn(&self, session_id: &str) {
        self.active_turns.lock().await.remove(session_id);
    }

    pub(crate) async fn update_turn_state(
        &self,
        session_id: &str,
        turn_id: &str,
        turn_state: TurnState,
    ) {
        let mut active_turns = self.active_turns.lock().await;
        if let Some(active_turn) = active_turns.get_mut(session_id)
            && active_turn.turn_id == turn_id
        {
            active_turn.turn_state = turn_state;
        }
    }

    pub(crate) async fn interrupt_session(&self, session_id: &str) -> bool {
        let active_turn = self.active_turn(session_id).await;
        if let Some(active_turn) = active_turn {
            active_turn.request_cancel();
            true
        } else {
            false
        }
    }
}
