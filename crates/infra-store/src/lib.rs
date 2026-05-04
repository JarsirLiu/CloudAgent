use agent_core::{
    ApprovalGrantKey, ApprovalGrantStoreBackend, ConversationStoreBackend, ConversationSummary,
};
use agent_core::{ResponseItem, RolloutItem, conversation_history_from_rollout_items};
use agent_protocol::EventMsg;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
pub mod memory_repo;
pub mod rollout_recorder;
mod session_index;

pub use rollout_recorder::RolloutRecorder;

#[derive(Clone, Debug)]
pub struct JsonConversationStore {
    root: PathBuf,
    io_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug)]
pub struct StoredConversationSummary {
    pub conversation_id: String,
    pub title: Option<String>,
    pub message_count: usize,
    pub updated_at_ms: u64,
    pub archived: bool,
}

#[async_trait]
impl ConversationStoreBackend for JsonConversationStore {
    async fn create_conversation(&self, conversation_id: &str) -> Result<()> {
        JsonConversationStore::create_conversation(self, conversation_id).await
    }

    async fn archive_conversation(&self, conversation_id: &str) -> Result<()> {
        JsonConversationStore::archive_conversation(self, conversation_id).await
    }

    async fn delete_conversation(&self, conversation_id: &str) -> Result<()> {
        JsonConversationStore::delete_conversation(self, conversation_id).await
    }

    async fn delete_events(&self, conversation_id: &str) -> Result<()> {
        JsonConversationStore::delete_events(self, conversation_id).await
    }

    async fn list_conversations(&self) -> Result<Vec<ConversationSummary>> {
        Ok(JsonConversationStore::list_conversations(self)
            .await?
            .into_iter()
            .map(|summary| ConversationSummary {
                conversation_id: summary.conversation_id,
                title: summary.title,
                message_count: summary.message_count,
                updated_at_ms: summary.updated_at_ms,
            })
            .collect())
    }

    async fn mark_active_conversation(&self, conversation_id: &str) -> Result<()> {
        JsonConversationStore::mark_active_conversation(self, conversation_id).await
    }

    async fn load_active_conversation(&self) -> Result<Option<String>> {
        JsonConversationStore::load_active_conversation(self).await
    }

    async fn set_conversation_title(&self, conversation_id: &str, title: &str) -> Result<()> {
        JsonConversationStore::set_conversation_title(self, conversation_id, title).await
    }

    async fn load_rollout_items(&self, conversation_id: &str) -> Result<Vec<RolloutItem>> {
        JsonConversationStore::load_rollout_items(self, conversation_id).await
    }

    async fn prune_archived_conversations_if_needed(&self) -> Result<()> {
        JsonConversationStore::prune_archived_conversations_if_needed(self).await
    }

    fn root(&self) -> &Path {
        JsonConversationStore::root(self)
    }
}

#[async_trait]
impl ApprovalGrantStoreBackend for JsonConversationStore {
    async fn has_approval_grant(
        &self,
        conversation_id: &str,
        key: &ApprovalGrantKey,
    ) -> Result<bool> {
        let grant_key_json = serde_json::to_string(key)?;
        session_index::has_approval_grant(
            &session_index::db_path(&self.root),
            conversation_id,
            &grant_key_json,
        )
    }

    async fn save_approval_grant(
        &self,
        conversation_id: &str,
        key: &ApprovalGrantKey,
    ) -> Result<()> {
        let grant_key_json = serde_json::to_string(key)?;
        session_index::upsert_approval_grant(
            &session_index::db_path(&self.root),
            conversation_id,
            &grant_key_json,
            now_ms(),
        )
    }
}

impl JsonConversationStore {
    const MAX_SESSION_BYTES: u64 = 2 * 1024 * 1024 * 1024;
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn delete_conversation(&self, conversation_id: &str) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        self.delete_file_if_exists(&self.rollout_path(conversation_id))
            .await?;
        session_index::delete_session(&session_index::db_path(&self.root), conversation_id)
    }

    pub async fn load_events(&self, conversation_id: &str) -> Result<Vec<EventMsg>> {
        Ok(self
            .load_rollout_items(conversation_id)
            .await?
            .into_iter()
            .filter_map(|item| match item {
                RolloutItem::EventMsg { event } => Some(event),
                RolloutItem::ResponseItem { .. } | RolloutItem::Compacted { .. } => None,
            })
            .collect())
    }

    pub async fn append_events(&self, conversation_id: &str, events: &[EventMsg]) -> Result<()> {
        let items = events
            .iter()
            .cloned()
            .map(RolloutItem::from)
            .collect::<Vec<_>>();
        self.append_rollout_items(conversation_id, &items).await
    }

    pub async fn load_rollout_items(&self, conversation_id: &str) -> Result<Vec<RolloutItem>> {
        let path = self.rollout_path(conversation_id);
        self.load_rollout_items_from_path(&path).await
    }

    pub async fn append_rollout_items(
        &self,
        conversation_id: &str,
        items: &[RolloutItem],
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let _guard = self.io_lock.lock().await;
        self.ensure_root_dir().await?;
        let path = self.rollout_path(conversation_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("failed to open {}", path.display()))?;
        for item in items {
            let line = serde_json::to_string(item)?;
            file.write_all(line.as_bytes())
                .await
                .with_context(|| format!("failed to append {}", path.display()))?;
            file.write_all(b"\n")
                .await
                .with_context(|| format!("failed to append newline to {}", path.display()))?;
        }
        file.flush()
            .await
            .with_context(|| format!("failed to flush {}", path.display()))?;
        self.refresh_session_summary_locked(conversation_id, false)
            .await?;
        Ok(())
    }

    pub async fn append_event(&self, conversation_id: &str, event: &EventMsg) -> Result<()> {
        self.append_events(conversation_id, std::slice::from_ref(event))
            .await
    }

    pub async fn delete_events(&self, conversation_id: &str) -> Result<()> {
        self.delete_file_if_exists(&self.event_path(conversation_id))
            .await
    }

    pub async fn create_conversation(&self, conversation_id: &str) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        self.ensure_root_dir().await?;
        let now = now_ms();
        let _ = session_index::upsert_session(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            conversation_id,
            0,
            now,
            false,
            None,
        );
        let _ = session_index::append_event(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            conversation_id,
            "create",
            None,
            "system",
            None,
            None,
            None,
            now,
        );
        Ok(())
    }

    pub async fn archive_conversation(&self, conversation_id: &str) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        self.ensure_root_dir().await?;
        self.refresh_session_summary_locked(conversation_id, true)
            .await?;
        let now = now_ms();
        let _ = session_index::append_event(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            conversation_id,
            "archive",
            None,
            "system",
            None,
            None,
            None,
            now,
        );
        self.prune_archived_conversations_to_limit_locked(Self::MAX_SESSION_BYTES)
            .await?;
        Ok(())
    }

    pub async fn prune_archived_conversations_if_needed(&self) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        self.ensure_root_dir().await?;
        self.prune_archived_conversations_to_limit_locked(Self::MAX_SESSION_BYTES)
            .await
    }

    pub async fn list_conversations(&self) -> Result<Vec<StoredConversationSummary>> {
        Ok(session_index::list_sessions(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
        )?
        .into_iter()
        .map(|row| StoredConversationSummary {
            conversation_id: row.conversation_id,
            title: row.title,
            message_count: row.message_count,
            updated_at_ms: row.updated_at_ms,
            archived: row.archived,
        })
        .collect())
    }

    pub async fn mark_active_conversation(&self, conversation_id: &str) -> Result<()> {
        let now = now_ms();
        session_index::mark_active(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            conversation_id,
            now,
        )?;
        session_index::append_event(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            conversation_id,
            "switch_active",
            None,
            "user",
            None,
            None,
            None,
            now,
        )?;
        Ok(())
    }

    pub async fn load_active_conversation(&self) -> Result<Option<String>> {
        session_index::get_active(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
        )
    }

    pub async fn set_conversation_title(&self, conversation_id: &str, title: &str) -> Result<()> {
        session_index::set_title(&session_index::db_path(&self.root), conversation_id, title)
    }

    pub async fn save_project_settings_snapshot(&self, config_json: &str) -> Result<()> {
        session_index::upsert_project_settings(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            config_json,
            now_ms(),
        )
    }

    pub async fn load_project_settings_snapshot(&self) -> Result<Option<String>> {
        session_index::get_project_settings(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
        )
    }

    fn event_path(&self, conversation_id: &str) -> PathBuf {
        self.rollout_path(conversation_id)
    }

    fn rollout_path(&self, conversation_id: &str) -> PathBuf {
        self.root.join(format!(
            "{}.rollout.jsonl",
            sanitize_conversation_id(conversation_id)
        ))
    }

    async fn load_rollout_items_from_path(&self, path: &Path) -> Result<Vec<RolloutItem>> {
        match self.read_rollout_log_text(path).await? {
            Some(text) => self.parse_rollout_log_text(path, &text),
            None => Ok(Vec::new()),
        }
    }

    async fn read_rollout_log_text(&self, path: &Path) -> Result<Option<String>> {
        match fs::read_to_string(path).await {
            Ok(text) => Ok(Some(text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    fn parse_rollout_log_text(&self, path: &Path, text: &str) -> Result<Vec<RolloutItem>> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let mut items = Vec::new();
        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let item = serde_json::from_str::<RolloutItem>(line).with_context(|| {
                format!(
                    "failed to parse rollout file {} at line {}",
                    path.display(),
                    line_no + 1
                )
            })?;
            items.push(item);
        }
        Ok(items)
    }

    async fn delete_file_if_exists(&self, path: &Path) -> Result<()> {
        match fs::remove_file(path).await {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("failed to delete {}", path.display())),
        }
    }

    async fn ensure_root_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("failed to create {}", self.root.display()))
    }

    async fn total_session_bytes_locked(&self) -> Result<u64> {
        let mut total = 0u64;
        let mut dir = fs::read_dir(&self.root).await?;
        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            let is_session_file = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".rollout.jsonl"))
                .unwrap_or(false);
            if is_session_file {
                total = total.saturating_add(entry.metadata().await?.len());
            }
        }
        Ok(total)
    }

    async fn prune_archived_conversations_to_limit_locked(&self, max_bytes: u64) -> Result<()> {
        let mut total = self.total_session_bytes_locked().await?;
        if total <= max_bytes {
            return Ok(());
        }
        let archived = session_index::list_archived_sessions(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
        )?;
        for summary in archived {
            if total <= max_bytes {
                break;
            }
            let rollout = self.rollout_path(&summary.conversation_id);
            let mut reclaimed = 0u64;
            if let Ok(meta) = fs::metadata(&rollout).await {
                reclaimed = reclaimed.saturating_add(meta.len());
            }
            self.delete_file_if_exists(&rollout).await?;
            session_index::delete_session(
                &session_index::db_path(&self.root),
                &summary.conversation_id,
            )?;
            total = total.saturating_sub(reclaimed);
        }
        Ok(())
    }

    async fn refresh_session_summary_locked(
        &self,
        conversation_id: &str,
        archived: bool,
    ) -> Result<()> {
        let rollout_items = self
            .load_rollout_items_from_path(&self.rollout_path(conversation_id))
            .await?;
        let history = conversation_history_from_rollout_items(
            conversation_id.to_string(),
            String::new(),
            &rollout_items,
        );
        let message_count = history
            .messages
            .iter()
            .filter(|message| match message {
                ResponseItem::User { content } => !content.trim().is_empty(),
                ResponseItem::Assistant { content, .. } => content
                    .as_deref()
                    .is_some_and(|content| !content.trim().is_empty()),
                ResponseItem::System { .. } | ResponseItem::Tool { .. } => false,
            })
            .count();
        session_index::upsert_session(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            conversation_id,
            message_count,
            now_ms(),
            archived,
            None,
        )
    }
}

pub fn save_project_settings_snapshot_sync(root: &Path, config_json: &str) -> Result<()> {
    session_index::upsert_project_settings(
        &session_index::db_path(root),
        &root.to_string_lossy(),
        config_json,
        now_ms(),
    )
}

pub fn load_project_settings_snapshot_sync(root: &Path) -> Result<Option<String>> {
    session_index::get_project_settings(&session_index::db_path(root), &root.to_string_lossy())
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sanitize_conversation_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ApprovalGrantKey;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn concurrent_event_appends_leave_valid_json() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cloudagent-storage-test-{unique}"));
        let store = JsonConversationStore::new(&root);
        let conversation_id = "concurrent-events";

        let mut tasks = Vec::new();
        for index in 0..8usize {
            let cloned = store.clone();
            tasks.push(tokio::spawn(async move {
                for item in 0..10usize {
                    cloned
                        .append_event(
                            conversation_id,
                            &EventMsg::TurnStarted {
                                turn_id: format!("turn-{index}-{item}"),
                                conversation_id: conversation_id.to_string(),
                                user_input: format!("message-{index}-{item}"),
                            },
                        )
                        .await
                        .expect("append event");
                }
            }));
        }

        for task in tasks {
            task.await.expect("append task");
        }

        let events = store
            .load_events(conversation_id)
            .await
            .expect("load events");
        assert_eq!(events.len(), 80);

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn pruning_removes_oldest_archived_conversations_first() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cloudagent-storage-prune-{unique}"));
        let store = JsonConversationStore::new(&root);
        store.ensure_root_dir().await.expect("create root");

        let now = now_ms();
        let entries = [
            ("archived-old", true, now.saturating_sub(3_000)),
            ("archived-new", true, now.saturating_sub(1_000)),
            ("active", false, now),
        ];
        for (conversation_id, archived, updated_at_ms) in entries {
            tokio::fs::write(store.rollout_path(conversation_id), "x".repeat(64 * 1024))
                .await
                .expect("write rollout");
            session_index::upsert_session(
                &session_index::db_path(&root),
                &root.to_string_lossy(),
                conversation_id,
                1,
                updated_at_ms,
                archived,
                None,
            )
            .expect("upsert session");
        }

        store
            .prune_archived_conversations_to_limit_locked(32 * 1024)
            .await
            .expect("prune");

        assert!(!store.rollout_path("archived-old").exists());
        assert!(!store.rollout_path("archived-new").exists());
        assert!(store.rollout_path("active").exists());

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn approval_grants_persist_across_store_restart() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cloudagent-approval-grants-{unique}"));
        let store = JsonConversationStore::new(&root);
        let conversation_id = "approval-session";
        let key = ApprovalGrantKey::new(
            "tool_session",
            json!({
                "identity": {
                    "source": "built_in",
                    "namespace": null,
                    "wire_name": "edit_file"
                }
            }),
        );

        store.ensure_root_dir().await.expect("create root");

        store
            .save_approval_grant(conversation_id, &key)
            .await
            .expect("save approval grant");

        let reopened = JsonConversationStore::new(&root);
        assert!(
            reopened
                .has_approval_grant(conversation_id, &key)
                .await
                .expect("load approval grant"),
            "approval grant should survive reopening the store"
        );

        let _ = fs::remove_dir_all(root).await;
    }
}
