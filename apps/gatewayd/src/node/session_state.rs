use crate::node::worker_manager::NodeEvent;
use tokio::sync::broadcast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportHandshakeState {
    AwaitingInitialize,
    AwaitingInitialized,
    Ready,
}

pub(crate) struct NodeSessionState {
    active_conversation_id: String,
    active_subscription: Option<broadcast::Receiver<NodeEvent>>,
    handshake_state: TransportHandshakeState,
}

impl NodeSessionState {
    pub(crate) fn new(active_conversation_id: impl Into<String>) -> Self {
        Self {
            active_conversation_id: active_conversation_id.into(),
            active_subscription: None,
            handshake_state: TransportHandshakeState::AwaitingInitialize,
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
