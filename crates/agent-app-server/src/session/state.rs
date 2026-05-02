use crate::routing::command_router::ServerState;
use agent_runtime::AgentRuntime;
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) struct ArchiveTransition {
    pub(crate) active_session_id: String,
    pub(crate) switched_active: bool,
}

pub(crate) async fn hydrate_active_conversation(
    runtime: &Arc<AgentRuntime>,
    state: &Arc<Mutex<ServerState>>,
) {
    if let Ok(Some(conversation_id)) = runtime.load_active_conversation().await {
        if conversation_id.trim().is_empty() {
            return;
        }
        apply_active_conversation(state, conversation_id).await;
    }
}

pub(crate) async fn apply_active_conversation(
    state: &Arc<Mutex<ServerState>>,
    conversation_id: String,
) {
    let mut guard = state.lock().await;
    guard.switch_active_conversation(conversation_id.clone());
    guard.subscribe(conversation_id);
}

pub(crate) async fn persist_active_conversation(
    runtime: &Arc<AgentRuntime>,
    conversation_id: &str,
) {
    let _ = runtime.mark_active_conversation(conversation_id).await;
}

pub(crate) async fn apply_archive_transition(
    state: &Arc<Mutex<ServerState>>,
    archived_conversation_id: &str,
    fallback_conversation_id: &str,
) -> ArchiveTransition {
    let mut guard = state.lock().await;
    let was_active = guard.active_conversation_id() == archived_conversation_id;
    guard.unsubscribe(archived_conversation_id);
    if was_active {
        let fallback = fallback_conversation_id.to_string();
        guard.switch_active_conversation(fallback.clone());
        guard.subscribe(fallback);
    }
    ArchiveTransition {
        active_session_id: guard.active_conversation_id().to_string(),
        switched_active: was_active,
    }
}

#[cfg(test)]
mod tests {
    use super::apply_archive_transition;
    use crate::routing::command_router::ServerState;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn archive_active_conversation_switches_to_default() {
        let state = Arc::new(Mutex::new(ServerState::new("default".to_string())));
        {
            let mut guard = state.lock().await;
            guard.switch_active_conversation("session-a".to_string());
            guard.subscribe("session-a".to_string());
        }

        let transition = apply_archive_transition(&state, "session-a", "default").await;
        assert!(transition.switched_active);
        assert_eq!(transition.active_session_id, "default");
    }

    #[tokio::test]
    async fn archive_inactive_conversation_keeps_active_unchanged() {
        let state = Arc::new(Mutex::new(ServerState::new("default".to_string())));
        {
            let mut guard = state.lock().await;
            guard.switch_active_conversation("session-a".to_string());
            guard.subscribe("session-a".to_string());
            guard.subscribe("session-b".to_string());
        }

        let transition = apply_archive_transition(&state, "session-b", "default").await;
        assert!(!transition.switched_active);
        assert_eq!(transition.active_session_id, "session-a");
    }
}
