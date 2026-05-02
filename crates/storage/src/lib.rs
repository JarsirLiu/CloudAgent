use agent_core::{ConversationState, PersistedConversation, RolloutItem};
use agent_protocol::EventMsg;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
mod session_index;

#[derive(Clone, Debug)]
pub struct JsonConversationStore {
    root: PathBuf,
    io_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredConversationSummary {
    pub conversation_id: String,
    pub title: Option<String>,
    pub message_count: usize,
    pub updated_at_ms: u64,
    pub archived: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct ConversationIndex {
    conversations: Vec<StoredConversationSummary>,
}

impl JsonConversationStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn load_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<ConversationState>> {
        let path = self.conversation_path(conversation_id);
        match fs::read_to_string(&path).await {
            Ok(text) => {
                let conversation = serde_json::from_str::<PersistedConversation>(&text)
                    .with_context(|| {
                        format!("failed to parse conversation file {}", path.display())
                    })?;
                Ok(Some(conversation.into_state()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    pub async fn save_conversation(&self, conversation: &ConversationState) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        self.ensure_root_dir().await?;
        let path = self.conversation_path(&conversation.history().id);
        let text = serde_json::to_string_pretty(&conversation.persisted_record())?;
        self.write_text_atomically(&path, &text).await?;
        self.upsert_index_entry_locked(StoredConversationSummary {
            conversation_id: conversation.history().id.clone(),
            title: None,
            message_count: conversation.history().messages.len(),
            updated_at_ms: now_ms(),
            archived: false,
        })
        .await?;
        let _ = session_index::upsert_session(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            &conversation.history().id,
            conversation.history().messages.len(),
            now_ms(),
            false,
            None,
        );
        Ok(())
    }

    pub async fn delete_conversation(&self, conversation_id: &str) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        self.delete_file_if_exists(&self.conversation_path(conversation_id))
            .await?;
        self.delete_file_if_exists(&self.rollout_path(conversation_id))
            .await?;
        self.remove_index_entry_locked(conversation_id).await
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
        self.touch_index_entry_locked(conversation_id).await?;
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
        self.upsert_index_entry_locked(StoredConversationSummary {
            conversation_id: conversation_id.to_string(),
            title: None,
            message_count: 0,
            updated_at_ms: now_ms(),
            archived: false,
        })
        .await?;
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
        let mut index = self.load_index_locked().await?;
        if let Some(summary) = index
            .conversations
            .iter_mut()
            .find(|summary| summary.conversation_id == conversation_id)
        {
            summary.archived = true;
            summary.updated_at_ms = now_ms();
        }
        self.save_index_locked(&index).await?;
        let now = now_ms();
        let _ = session_index::upsert_session(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            conversation_id,
            0,
            now,
            true,
            None,
        );
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
        Ok(())
    }

    pub async fn list_conversations(&self) -> Result<Vec<StoredConversationSummary>> {
        if let Ok(index_rows) =
            session_index::list_sessions(&session_index::db_path(&self.root), &self.root.to_string_lossy())
        {
            return Ok(index_rows
                .into_iter()
                .map(|row| StoredConversationSummary {
                    conversation_id: row.conversation_id,
                    title: row.title,
                    message_count: row.message_count,
                    updated_at_ms: row.updated_at_ms,
                    archived: row.archived,
                })
                .collect());
        }
        let _guard = self.io_lock.lock().await;
        let mut conversations = self
            .load_index_locked()
            .await?
            .conversations
            .into_iter()
            .filter(|summary| !summary.archived)
            .collect::<Vec<_>>();
        conversations.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| a.conversation_id.cmp(&b.conversation_id))
        });
        Ok(conversations)
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

    fn conversation_path(&self, conversation_id: &str) -> PathBuf {
        self.root.join(format!(
            "{}.conversation.json",
            sanitize_conversation_id(conversation_id)
        ))
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

    fn index_path(&self) -> PathBuf {
        self.root.join("conversations.index.json")
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

    async fn write_text_atomically(&self, path: &Path, text: &str) -> Result<()> {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid file name for {}", path.display()))?;
        let temp_path = path.with_file_name(format!("{file_name}.tmp"));
        fs::write(&temp_path, text)
            .await
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        if fs::try_exists(path).await.unwrap_or(false) {
            fs::remove_file(path)
                .await
                .with_context(|| format!("failed to replace {}", path.display()))?;
        }
        fs::rename(&temp_path, path).await.with_context(|| {
            format!(
                "failed to rename {} to {}",
                temp_path.display(),
                path.display()
            )
        })?;
        Ok(())
    }

    async fn ensure_root_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("failed to create {}", self.root.display()))
    }

    async fn load_index_locked(&self) -> Result<ConversationIndex> {
        let path = self.index_path();
        match fs::read_to_string(&path).await {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("failed to parse {}", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConversationIndex::default())
            }
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    async fn save_index_locked(&self, index: &ConversationIndex) -> Result<()> {
        let text = serde_json::to_string_pretty(index)?;
        self.write_text_atomically(&self.index_path(), &text).await
    }

    async fn upsert_index_entry_locked(&self, summary: StoredConversationSummary) -> Result<()> {
        let mut index = self.load_index_locked().await?;
        if let Some(existing) = index
            .conversations
            .iter_mut()
            .find(|existing| existing.conversation_id == summary.conversation_id)
        {
            *existing = summary;
        } else {
            index.conversations.push(summary);
        }
        self.save_index_locked(&index).await
    }

    async fn touch_index_entry_locked(&self, conversation_id: &str) -> Result<()> {
        let mut index = self.load_index_locked().await?;
        if let Some(existing) = index
            .conversations
            .iter_mut()
            .find(|existing| existing.conversation_id == conversation_id)
        {
            existing.updated_at_ms = now_ms();
            existing.archived = false;
        } else {
            index.conversations.push(StoredConversationSummary {
                conversation_id: conversation_id.to_string(),
                title: None,
                message_count: 0,
                updated_at_ms: now_ms(),
                archived: false,
            });
        }
        self.save_index_locked(&index).await
    }

    async fn remove_index_entry_locked(&self, conversation_id: &str) -> Result<()> {
        let mut index = self.load_index_locked().await?;
        index
            .conversations
            .retain(|summary| summary.conversation_id != conversation_id);
        self.save_index_locked(&index).await
    }
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
    use agent_core::ConversationHistory;
    use agent_protocol::{RequestId, ServerRequest, ToolApprovalRequest};
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
    async fn conversation_roundtrips_pending_requests() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cloudagent-conversation-test-{unique}"));
        let store = JsonConversationStore::new(&root);

        let mut conversation =
            ConversationState::new(ConversationHistory::new("default", "system"));
        conversation.context_mut().record_user_message("hello");
        conversation.set_pending_request(
            RequestId::Integer(1),
            ServerRequest::ToolApproval {
                request: ToolApprovalRequest {
                    turn_id: "turn-1".to_string(),
                    tool_call_id: "call-1".to_string(),
                    tool_name: "shell_command".to_string(),
                    reason: "need pwd".to_string(),
                    arguments_preview: "{\"command\":\"pwd\"}".to_string(),
                },
            },
        );

        store
            .save_conversation(&conversation)
            .await
            .expect("save conversation");
        let loaded = store
            .load_conversation("default")
            .await
            .expect("load conversation")
            .expect("conversation exists");

        assert_eq!(loaded.history().messages.len(), 2);
        assert_eq!(loaded.pending_requests.len(), 1);

        let _ = fs::remove_dir_all(root).await;
    }
}
