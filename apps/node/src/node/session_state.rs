use crate::node::source::NodeSource;
use crate::node::worker_manager::NodeEvent;
use std::collections::HashSet;

use agent_protocol::{AppServerMessage, AppServerNotification};
use tokio::sync::broadcast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportHandshakeState {
    AwaitingInitialize,
    AwaitingInitialized,
    Ready,
}

pub(crate) struct NodeSessionState {
    source: NodeSource,
    active_conversation_id: String,
    subscribed_conversations: HashSet<String>,
    active_subscription: Option<broadcast::Receiver<NodeEvent>>,
    handshake_state: TransportHandshakeState,
}

impl NodeSessionState {
    pub(crate) fn new(
        active_conversation_id: impl Into<String>,
        worker_scope_key: impl Into<String>,
    ) -> Self {
        let active_conversation_id = active_conversation_id.into();
        let mut subscribed_conversations = HashSet::new();
        subscribed_conversations.insert(active_conversation_id.clone());
        Self {
            source: NodeSource::placeholder(worker_scope_key),
            active_conversation_id,
            subscribed_conversations,
            active_subscription: None,
            handshake_state: TransportHandshakeState::AwaitingInitialize,
        }
    }

    pub(crate) fn set_source(&mut self, source: NodeSource) {
        self.source = source;
    }

    pub(crate) fn worker_scope_key(&self) -> &str {
        self.source.worker_scope_key()
    }

    pub(crate) fn source_domain_id(&self) -> &str {
        self.source.domain_id()
    }

    pub(crate) fn active_conversation_id(&self) -> &str {
        &self.active_conversation_id
    }

    pub(crate) fn set_active_conversation_id(&mut self, conversation_id: String) {
        self.active_conversation_id = conversation_id;
    }

    pub(crate) fn subscribe_conversation(&mut self, conversation_id: impl Into<String>) {
        self.subscribed_conversations.insert(conversation_id.into());
    }

    pub(crate) fn unsubscribe_conversation(&mut self, conversation_id: &str) {
        self.subscribed_conversations.remove(conversation_id);
    }

    pub(crate) fn should_forward_event(&self, event: &NodeEvent) -> bool {
        match event {
            NodeEvent::Diagnostic { .. } => true,
            NodeEvent::Message { message } => self.should_forward_message(message),
        }
    }

    fn should_forward_message(&self, message: &AppServerMessage) -> bool {
        if matches!(
            message,
            AppServerMessage::Notification(
                AppServerNotification::ConversationSubscriptionChanged {
                    subscribed: false,
                    ..
                }
            )
        ) {
            return true;
        }
        message
            .conversation_id()
            .is_none_or(|conversation_id| self.subscribed_conversations.contains(conversation_id))
    }

    pub(crate) fn active_subscription_mut(
        &mut self,
    ) -> &mut Option<broadcast::Receiver<NodeEvent>> {
        &mut self.active_subscription
    }

    pub(crate) fn expects_initialize(&self) -> bool {
        self.handshake_state == TransportHandshakeState::AwaitingInitialize
    }

    pub(crate) fn expects_initialized_notification(&self) -> bool {
        self.handshake_state == TransportHandshakeState::AwaitingInitialized
    }

    pub(crate) fn mark_initialize_accepted(&mut self) {
        self.handshake_state = TransportHandshakeState::AwaitingInitialized;
    }

    pub(crate) fn mark_ready(&mut self) {
        self.handshake_state = TransportHandshakeState::Ready;
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.handshake_state == TransportHandshakeState::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::NodeSessionState;
    use crate::node::worker_manager::NodeEvent;
    use agent_protocol::{AppServerMessage, AppServerNotification};

    #[test]
    fn unsubscribe_ack_is_forwarded_even_after_local_unsubscribe() {
        let mut session = NodeSessionState::new("conversation-1", "session-1");
        session.unsubscribe_conversation("conversation-1");

        let event = NodeEvent::Message {
            message: Box::new(AppServerMessage::Notification(
                AppServerNotification::ConversationSubscriptionChanged {
                    conversation_id: "conversation-1".to_string(),
                    subscribed: false,
                },
            )),
        };

        assert!(session.should_forward_event(&event));
    }
}
