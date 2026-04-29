use std::collections::HashSet;

#[derive(Debug)]
pub(crate) struct SessionSubscriptions {
    subscribed_sessions: HashSet<String>,
}

impl SessionSubscriptions {
    pub(crate) fn new(default_session_id: String) -> Self {
        let mut subscribed_sessions = HashSet::new();
        subscribed_sessions.insert(default_session_id);
        Self { subscribed_sessions }
    }

    pub(crate) fn is_subscribed(&self, session_id: &str) -> bool {
        self.subscribed_sessions.contains(session_id)
    }

    pub(crate) fn subscribe(&mut self, session_id: String) {
        self.subscribed_sessions.insert(session_id);
    }

    pub(crate) fn unsubscribe(&mut self, session_id: &str) {
        self.subscribed_sessions.remove(session_id);
    }
}

