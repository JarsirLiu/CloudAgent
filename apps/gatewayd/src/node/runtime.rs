use crate::node::conversation_registry::ConversationRegistry;
use crate::node::platform::PlatformManager;
use crate::node::worker_manager::WorkerManager;
use agent_protocol::NodeStatusResponse;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

#[derive(Clone)]
pub(crate) struct NodeRuntime {
    workers: WorkerManager,
    conversations: Arc<Mutex<ConversationRegistry>>,
    platforms: PlatformManager,
    listen_address: String,
    shutdown: Arc<Notify>,
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
            shutdown: Arc::new(Notify::new()),
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

    pub(crate) async fn status(&self) -> NodeStatusResponse {
        NodeStatusResponse {
            listen_address: self.listen_address.clone(),
            worker_running: self.workers.is_worker_running().await,
            platform_runtime_count: self.platforms.runtime_count().await,
            managed_platform_count: self.platforms.managed_platform_count(),
        }
    }

    pub(crate) fn request_shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    pub(crate) async fn wait_for_shutdown(&self) {
        self.shutdown.notified().await;
    }

    pub(crate) async fn shutdown(&self) -> anyhow::Result<()> {
        self.workers.shutdown().await
    }
}
