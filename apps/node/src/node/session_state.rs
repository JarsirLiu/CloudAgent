use crate::node::source::NodeSource;
use crate::node::worker_manager::NodeEvent;
use std::collections::HashSet;
use std::path::PathBuf;

use agent_protocol::{
    AppServerMessage, AppServerNotification, CommandExecutionContext, SessionBootstrapContext,
};
use tokio::sync::broadcast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportHandshakeState {
    AwaitingInitialize,
    AwaitingInitialized,
    Ready,
}

pub(crate) struct NodeSessionState {
    source: NodeSource,
    worker_scope_key: String,
    session_id: Option<String>,
    workspace_root: Option<PathBuf>,
    cwd: Option<PathBuf>,
    permission_mode: Option<String>,
    data_root_dir: Option<PathBuf>,
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
        let worker_scope_key = worker_scope_key.into();
        let active_conversation_id = active_conversation_id.into();
        let mut subscribed_conversations = HashSet::new();
        subscribed_conversations.insert(active_conversation_id.clone());
        let mut state = Self {
            source: NodeSource::placeholder(worker_scope_key),
            worker_scope_key: String::new(),
            session_id: None,
            workspace_root: None,
            cwd: None,
            permission_mode: None,
            data_root_dir: None,
            active_conversation_id,
            subscribed_conversations,
            active_subscription: None,
            handshake_state: TransportHandshakeState::AwaitingInitialize,
        };
        state.recompute_worker_scope();
        state
    }

    pub(crate) fn set_source(&mut self, source: NodeSource) {
        self.source = source;
        self.recompute_worker_scope();
    }

    pub(crate) fn worker_scope_key(&self) -> &str {
        &self.worker_scope_key
    }

    pub(crate) fn source_domain_id(&self) -> &str {
        self.source.domain_id()
    }

    pub(crate) fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub(crate) fn workspace_root(&self) -> Option<&std::path::Path> {
        self.workspace_root.as_deref()
    }

    pub(crate) fn cwd(&self) -> Option<&std::path::Path> {
        self.cwd.as_deref()
    }

    pub(crate) fn permission_mode(&self) -> Option<&str> {
        self.permission_mode.as_deref()
    }

    pub(crate) fn data_root_dir(&self) -> Option<&std::path::Path> {
        self.data_root_dir.as_deref()
    }

    pub(crate) fn apply_session_context(&mut self, context: &SessionBootstrapContext) {
        if let Some(session_id) = &context.session_id {
            self.session_id = Some(session_id.clone());
        }
        if let Some(workspace_root) = &context.workspace_root {
            self.workspace_root = Some(PathBuf::from(workspace_root));
        }
        if let Some(cwd) = &context.cwd {
            self.cwd = Some(PathBuf::from(cwd));
        }
        if let Some(permission_mode) = &context.permission_mode {
            self.permission_mode = Some(permission_mode.clone());
        }
        if let Some(data_root_dir) = &context.data_root_dir {
            self.data_root_dir = Some(PathBuf::from(data_root_dir));
        }
        self.recompute_worker_scope();
    }

    pub(crate) fn apply_command_context(&mut self, context: Option<&CommandExecutionContext>) {
        let Some(context) = context else {
            return;
        };
        if let Some(session_id) = &context.session_id {
            self.session_id = Some(session_id.clone());
        }
        if let Some(workspace_root) = &context.workspace_root {
            self.workspace_root = Some(PathBuf::from(workspace_root));
        }
        if let Some(cwd) = &context.cwd {
            self.cwd = Some(PathBuf::from(cwd));
        }
        if let Some(permission_mode) = &context.permission_mode {
            self.permission_mode = Some(permission_mode.clone());
        }
        if let Some(data_root_dir) = &context.data_root_dir {
            self.data_root_dir = Some(PathBuf::from(data_root_dir));
        }
        self.recompute_worker_scope();
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

    fn recompute_worker_scope(&mut self) {
        self.worker_scope_key = self.source.worker_scope_key(self.workspace_root.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::NodeSessionState;
    use crate::node::worker_manager::NodeEvent;
    use agent_protocol::{
        AppServerMessage, AppServerNotification, ConversationViewSnapshot, ConversationViewStatus,
    };

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

    #[test]
    fn conversation_view_changed_only_forwards_subscribed_conversations() {
        let session = NodeSessionState::new("conversation-1", "session-1");

        assert!(session.should_forward_event(&conversation_view_event("conversation-1")));
        assert!(!session.should_forward_event(&conversation_view_event("conversation-2")));
    }

    #[test]
    fn subscription_allows_additional_conversation_view_events() {
        let mut session = NodeSessionState::new("conversation-1", "session-1");

        session.subscribe_conversation("conversation-2");

        assert!(session.should_forward_event(&conversation_view_event("conversation-2")));
    }

    fn conversation_view_event(conversation_id: &str) -> NodeEvent {
        NodeEvent::Message {
            message: Box::new(AppServerMessage::Notification(
                AppServerNotification::ConversationViewChanged {
                    conversation_id: conversation_id.to_string(),
                    snapshot: ConversationViewSnapshot {
                        conversation_id: conversation_id.to_string(),
                        status: ConversationViewStatus::Idle,
                        active_turn: None,
                        pending_requests: Vec::new(),
                        message_count: 0,
                        updated_at_ms: 0,
                    },
                },
            )),
        }
    }
}
