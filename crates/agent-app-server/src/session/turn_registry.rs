use crate::session::listener::ConversationListenerHandle;
use std::collections::HashMap;
use tokio::task::JoinHandle;

#[derive(Default)]
pub(crate) struct SessionTurnRegistry {
    turn_tasks_by_conversation: HashMap<String, Vec<JoinHandle<()>>>,
    active_listeners: HashMap<String, ConversationListenerHandle>,
}

impl SessionTurnRegistry {
    pub(crate) fn track_turn_task(&mut self, conversation_id: String, task: JoinHandle<()>) {
        let tasks = self
            .turn_tasks_by_conversation
            .entry(conversation_id)
            .or_default();
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }

    pub(crate) fn take_turn_tasks_for_conversation(
        &mut self,
        conversation_id: &str,
    ) -> Vec<JoinHandle<()>> {
        self.turn_tasks_by_conversation
            .remove(conversation_id)
            .unwrap_or_default()
    }

    pub(crate) fn take_all_turn_tasks(&mut self) -> Vec<JoinHandle<()>> {
        self.turn_tasks_by_conversation
            .drain()
            .flat_map(|(_, tasks)| tasks)
            .collect()
    }

    pub(crate) fn set_active_listener(
        &mut self,
        conversation_id: String,
        listener: ConversationListenerHandle,
    ) {
        self.active_listeners.insert(conversation_id, listener);
    }

    pub(crate) fn clear_active_listener(&mut self, conversation_id: &str) {
        self.active_listeners.remove(conversation_id);
    }

    pub(crate) fn active_listener(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationListenerHandle> {
        self.active_listeners.get(conversation_id).cloned()
    }
}
