use crate::node::conversation_registry::ConversationRegistry;
use crate::node::platform_manager::PlatformManager;
use crate::node::worker_manager::WorkerManager;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub(crate) struct NodeRuntime {
    workers: WorkerManager,
    conversations: Arc<Mutex<ConversationRegistry>>,
    platforms: PlatformManager,
    listen_address: String,
}

impl NodeRuntime {
    pub(crate) fn new(
        workers: WorkerManager,
        platforms: PlatformManager,
        listen_address: impl Into<String>,
    ) -> Self {
        Self {
            workers,
            conversations: Arc::new(Mutex::new(ConversationRegistry::default())),
            platforms,
            listen_address: listen_address.into(),
        }
    }

    pub(crate) fn workers(&self) -> &WorkerManager {
        &self.workers
    }

    pub(crate) fn conversations(&self) -> &Arc<Mutex<ConversationRegistry>> {
        &self.conversations
    }

    pub(crate) fn platforms(&self) -> &PlatformManager {
        &self.platforms
    }

    pub(crate) fn listen_address(&self) -> &str {
        &self.listen_address
    }
}
