mod agent;

use crate::{
    AgentContext, AgentState, ApprovalGrantStoreBackend, ApprovalPolicy, ChatModel,
    ChatModelFactory, ChatTurnSettings, ConversationHistory, ConversationSummary, ExecutionPolicy,
    PermissionProfile, ReloadableChatModel, RolloutItem, SkillRuntime, ToolBackend,
};
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use agent::{AgentHost, timestamp_conversation_id};

#[derive(Clone, Debug)]
pub struct AgentMetadata {
    pub llm_model_name: String,
    pub conversation_store_dir: PathBuf,
    pub cli_pre_llm_filter_enabled: bool,
    pub cli_permission_mode: String,
    pub shell_name: String,
    pub system_prompt: String,
}

pub struct AgentHostParts {
    pub metadata: AgentMetadata,
    pub context: AgentContext,
    pub chat_turn_settings: ChatTurnSettings,
    pub policy: ExecutionPolicy,
    pub model: Arc<dyn ChatModel>,
    pub reloadable_model: Option<Arc<ReloadableChatModel>>,
    pub model_factory: Option<Arc<dyn ChatModelFactory>>,
    pub tools: Arc<
        dyn ToolBackend<PermissionProfile = PermissionProfile, ApprovalPolicy = ApprovalPolicy>,
    >,
    pub state: AgentState,
    pub store: Arc<dyn ConversationStoreBackend>,
    pub approval_grants: Arc<dyn ApprovalGrantStoreBackend>,
    pub rollout_recorder: Arc<dyn RolloutRecorderBackend>,
    pub memory: Arc<dyn MemoryBackend>,
    pub skills: SkillRuntime,
}

pub trait AgentHostExt {
    fn metadata(&self) -> &AgentMetadata;
    fn context(&self) -> &AgentContext;
    fn chat_turn_settings(&self) -> &ChatTurnSettings;
    fn policy(&self) -> &ExecutionPolicy;
    fn model(&self) -> &Arc<dyn ChatModel>;
    fn tools(
        &self,
    ) -> &Arc<dyn ToolBackend<PermissionProfile = PermissionProfile, ApprovalPolicy = ApprovalPolicy>>;
    fn state(&self) -> &AgentState;
    fn store(&self) -> &Arc<dyn ConversationStoreBackend>;
    fn approval_grants(&self) -> &Arc<dyn ApprovalGrantStoreBackend>;
    fn rollout_recorder(&self) -> &Arc<dyn RolloutRecorderBackend>;
    fn memory(&self) -> &Arc<dyn MemoryBackend>;
    fn skills(&self) -> &SkillRuntime;

    fn llm_model_name(&self) -> &str {
        &self.metadata().llm_model_name
    }

    fn conversation_store_dir(&self) -> &Path {
        &self.metadata().conversation_store_dir
    }

    fn cli_pre_llm_filter_enabled(&self) -> bool {
        self.metadata().cli_pre_llm_filter_enabled
    }

    fn cli_permission_mode(&self) -> &str {
        &self.metadata().cli_permission_mode
    }
}

#[async_trait]
pub trait ConversationStoreBackend: Send + Sync {
    async fn create_conversation(&self, conversation_id: &str) -> Result<()>;
    async fn has_conversation(&self, conversation_id: &str) -> Result<bool>;
    async fn archive_conversation(&self, conversation_id: &str) -> Result<()>;
    async fn delete_conversation(&self, conversation_id: &str) -> Result<()>;
    async fn delete_events(&self, conversation_id: &str) -> Result<()>;
    async fn list_conversations(&self) -> Result<Vec<ConversationSummary>>;
    async fn list_conversations_page(
        &self,
        cursor: Option<String>,
        limit: usize,
    ) -> Result<ConversationListPage>;
    async fn reconcile_missing_conversations(
        &self,
        limit: usize,
    ) -> Result<ConversationReconcileReport>;
    async fn purge_missing_conversation_if_needed(&self, conversation_id: &str) -> Result<bool>;
    async fn mark_active_conversation(&self, conversation_id: &str) -> Result<()>;
    async fn load_active_conversation(&self) -> Result<Option<String>>;
    async fn set_conversation_title(&self, conversation_id: &str, title: &str) -> Result<()>;
    async fn load_rollout_items(&self, conversation_id: &str) -> Result<Vec<RolloutItem>>;
    async fn load_rollout_items_page(
        &self,
        conversation_id: &str,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<RolloutItemsPage>;
    async fn prune_archived_conversations_if_needed(&self) -> Result<()>;
    fn root(&self) -> &Path;
}

pub struct RolloutItemsPage {
    pub items: Vec<RolloutItem>,
    pub has_more: bool,
}

pub struct ConversationListPage {
    pub conversations: Vec<ConversationSummary>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

pub struct ConversationReconcileReport {
    pub checked: usize,
    pub removed: Vec<String>,
    pub truncated: bool,
}

#[async_trait]
pub trait RolloutRecorderBackend: Send + Sync {
    fn record_items(&self, conversation_id: &str, items: &[RolloutItem]) -> Result<()>;
    async fn flush(&self) -> Result<()>;
}

pub trait MemoryBackend: Send + Sync {
    fn raw_memory_fragment(&self) -> Option<String>;
    fn should_persist(&self, history: &ConversationHistory) -> bool;
    fn persist_from_history(&self, history: &ConversationHistory) -> Result<()>;
}
