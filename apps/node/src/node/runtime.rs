use crate::node::conversation_execution::ConversationExecutionRegistry;
use crate::node::conversation_registry::ConversationRegistry;
use crate::node::platform::PlatformManager;
use crate::node::worker_manager::WorkerManager;
use agent_core::{SkillMetadata, SkillRuntime};
use agent_protocol::NodeStatusResponse;
use infra_store::JsonConversationStore;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::Notify;

#[derive(Clone)]
pub(crate) struct NodeRuntime {
    workers: WorkerManager,
    conversations: Arc<Mutex<ConversationRegistry>>,
    executions: Arc<Mutex<ConversationExecutionRegistry>>,
    conversation_store: Arc<JsonConversationStore>,
    platforms: PlatformManager,
    listen_address: String,
    workspace_root: PathBuf,
    skills: SkillRuntime,
    data_root_dir: PathBuf,
    shutdown: Arc<Notify>,
}

impl NodeRuntime {
    pub(crate) fn new(
        workers: WorkerManager,
        conversation_store: JsonConversationStore,
        platforms: PlatformManager,
        listen_address: impl Into<String>,
        workspace_root: PathBuf,
        skills: SkillRuntime,
        data_root_dir: PathBuf,
    ) -> Self {
        Self {
            workers,
            conversations: Arc::new(Mutex::new(ConversationRegistry::default())),
            executions: Arc::new(Mutex::new(ConversationExecutionRegistry::default())),
            conversation_store: Arc::new(conversation_store),
            platforms,
            listen_address: listen_address.into(),
            workspace_root,
            skills,
            data_root_dir,
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub(crate) fn workers(&self) -> &WorkerManager {
        &self.workers
    }

    pub(crate) fn conversations(&self) -> &Arc<Mutex<ConversationRegistry>> {
        &self.conversations
    }

    pub(crate) fn executions(&self) -> &Arc<Mutex<ConversationExecutionRegistry>> {
        &self.executions
    }

    pub(crate) fn conversation_store(&self) -> &Arc<JsonConversationStore> {
        &self.conversation_store
    }

    pub(crate) async fn is_conversation_busy(&self, conversation_id: &str) -> bool {
        self.executions().lock().await.is_busy(conversation_id)
    }

    pub(crate) fn platforms(&self) -> &PlatformManager {
        &self.platforms
    }

    pub(crate) fn listen_address(&self) -> &str {
        &self.listen_address
    }

    pub(crate) fn list_skills(&self) -> Vec<SkillMetadata> {
        self.skills.load_catalog(&self.workspace_root).skills
    }

    pub(crate) async fn status(&self) -> NodeStatusResponse {
        NodeStatusResponse {
            listen_address: self.listen_address.clone(),
            worker_running: self.workers.is_worker_running().await,
            platform_runtime_count: self.platforms.runtime_count().await,
            managed_platform_count: self.platforms.managed_platform_count(),
            data_root_dir: self.data_root_dir.to_string_lossy().into_owned(),
            conversation_store_dir: self
                .conversation_store
                .root()
                .to_string_lossy()
                .into_owned(),
            workers: self.workers.status_snapshot().await,
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
