use agent_core::{ConversationState, PersistedConversation, RolloutItem};
use agent_protocol::EventMsg;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct JsonConversationStore {
    root: PathBuf,
    io_lock: Arc<Mutex<()>>,
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
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let path = self.conversation_path(&conversation.history().id);
        let text = serde_json::to_string_pretty(&conversation.persisted_record())?;
        self.write_text_atomically(&path, &text).await?;
        Ok(())
    }

    pub async fn delete_conversation(&self, conversation_id: &str) -> Result<()> {
        self.delete_file_if_exists(&self.conversation_path(conversation_id))
            .await
    }

    pub async fn load_events(&self, conversation_id: &str) -> Result<Vec<EventMsg>> {
        Ok(self
            .load_rollout_items(conversation_id)
            .await?
            .into_iter()
            .filter_map(|item| match item {
                RolloutItem::EventMsg { event } => Some(event),
                RolloutItem::ResponseItem { .. }
                | RolloutItem::Compacted { .. }
                | RolloutItem::TurnContext { .. }
                | RolloutItem::SessionMeta { .. } => None,
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
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("failed to create {}", self.root.display()))?;
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
