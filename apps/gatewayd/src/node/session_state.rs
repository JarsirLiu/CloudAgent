use crate::node::worker_manager::NodeEvent;
use tokio::sync::broadcast;

pub(crate) struct NodeSessionState {
    active_conversation_id: String,
    active_subscription: Option<broadcast::Receiver<NodeEvent>>,
}

impl NodeSessionState {
    pub(crate) fn new(active_conversation_id: impl Into<String>) -> Self {
        Self {
            active_conversation_id: active_conversation_id.into(),
            active_subscription: None,
        }
    }

    pub(crate) fn active_conversation_id(&self) -> &str {
        &self.active_conversation_id
    }

    pub(crate) fn set_active_conversation_id(&mut self, conversation_id: String) {
        self.active_conversation_id = conversation_id;
    }

    pub(crate) fn active_subscription_mut(
        &mut self,
    ) -> &mut Option<broadcast::Receiver<NodeEvent>> {
        &mut self.active_subscription
    }
}
