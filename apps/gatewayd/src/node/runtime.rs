use crate::node::conversation_registry::ConversationRegistry;
use crate::node::worker_manager::WorkerManager;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub(crate) struct NodeRuntime {
    workers: WorkerManager,
    conversations: Arc<Mutex<ConversationRegistry>>,
}

impl NodeRuntime {
    pub(crate) fn new(workers: WorkerManager) -> Self {
        Self {
            workers,
            conversations: Arc::new(Mutex::new(ConversationRegistry::default())),
        }
    }

    pub(crate) fn workers(&self) -> &WorkerManager {
        &self.workers
    }

    pub(crate) fn conversations(&self) -> &Arc<Mutex<ConversationRegistry>> {
        &self.conversations
    }
}
