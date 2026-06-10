use agent_core::EventMsg;
use agent_core::approval::ApprovalGrantStoreBackend;
use agent_core::conversation::{ConversationSummary, ResponseItem, input_items_are_blank};
use agent_core::host::{
    ConversationListPage, ConversationReconcileReport, ConversationStoreBackend, RolloutItemsPage,
};
use agent_core::projection::conversation_history_from_rollout_items;
use agent_core::rollout::RolloutItem;
use agent_core::tool::ApprovalGrantKey;
use agent_core::{RolloutPersistenceMode, persisted_rollout_items};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
pub mod memory_repo;
mod rollout_log;
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

fn stored_summaries_to_protocol(
    summaries: Vec<StoredConversationSummary>,
) -> Vec<ConversationSummary> {
    summaries
        .into_iter()
        .map(|summary| ConversationSummary {
            conversation_id: summary.conversation_id,
            title: summary.title,
            message_count: summary.message_count,
            updated_at_ms: summary.updated_at_ms,
        })
        .collect()
}

struct RolloutTurnChunk {
    items: Vec<RolloutItem>,
}

fn rollout_turn_start_id(item: &RolloutItem) -> Option<&str> {
    match item {
        RolloutItem::EventMsg {
            event: EventMsg::TurnStarted { turn_id, .. },
        } => Some(turn_id),
        RolloutItem::EventMsg { .. }
        | RolloutItem::ResponseItem { .. }
        | RolloutItem::Compacted { .. } => None,
    }
}

fn push_turn_page_chunk(
    window: &mut VecDeque<RolloutTurnChunk>,
    chunk: Option<RolloutTurnChunk>,
    page_limit: usize,
) {
    let Some(chunk) = chunk else {
        return;
    };
    window.push_back(chunk);
    while window.len() > page_limit.saturating_add(1) {
        window.pop_front();
    }
}

#[async_trait]
impl ConversationStoreBackend for JsonConversationStore {
    async fn create_conversation(&self, conversation_id: &str) -> Result<()> {
        JsonConversationStore::create_conversation(self, conversation_id).await
    }

    async fn has_conversation(&self, conversation_id: &str) -> Result<bool> {
        JsonConversationStore::has_conversation(self, conversation_id).await
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
        Ok(stored_summaries_to_protocol(
            JsonConversationStore::list_conversations(self).await?,
        ))
    }

    async fn list_conversations_page(
        &self,
        cursor: Option<String>,
        limit: usize,
    ) -> Result<ConversationListPage> {
        JsonConversationStore::list_conversations_page(self, cursor, limit).await
    }

    async fn reconcile_missing_conversations(
        &self,
        limit: usize,
    ) -> Result<ConversationReconcileReport> {
        JsonConversationStore::reconcile_missing_conversations(self, limit).await
    }

    async fn purge_missing_conversation_if_needed(&self, conversation_id: &str) -> Result<bool> {
        JsonConversationStore::purge_missing_conversation_if_needed(self, conversation_id).await
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

    async fn load_rollout_items_page(
        &self,
        conversation_id: &str,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<RolloutItemsPage> {
        JsonConversationStore::load_rollout_items_page(self, conversation_id, before_turn_id, limit)
            .await
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

    pub async fn load_rollout_items_page(
        &self,
        conversation_id: &str,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<RolloutItemsPage> {
        let path = self.rollout_path(conversation_id);
        self.load_rollout_items_page_from_path(&path, conversation_id, before_turn_id, limit)
            .await
    }

    pub async fn append_rollout_items(
        &self,
        conversation_id: &str,
        items: &[RolloutItem],
    ) -> Result<()> {
        let items = persisted_rollout_items(items, RolloutPersistenceMode::Limited);
        if items.is_empty() {
            return Ok(());
        }
        let _guard = self.io_lock.lock().await;
        self.ensure_root_dir().await?;
        let path = self.rollout_path(conversation_id);
        let mut next_offset = match fs::metadata(&path).await {
            Ok(metadata) => metadata.len(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
            Err(err) => {
                return Err(err).with_context(|| format!("failed to stat {}", path.display()));
            }
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("failed to open {}", path.display()))?;
        let mut turn_index_rows = Vec::new();
        for item in &items {
            let line_start_offset = next_offset;
            let line = serde_json::to_string(item)?;
            next_offset = next_offset
                .saturating_add(line.len() as u64)
                .saturating_add(1);
            if let Some(turn_id) = rollout_turn_start_id(item) {
                turn_index_rows.push(session_index::TurnIndexRow {
                    turn_id: turn_id.to_string(),
                    start_offset: line_start_offset,
                });
            }
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
        session_index::append_turn_index_rows(
            &session_index::db_path(&self.root),
            conversation_id,
            &turn_index_rows,
        )?;
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

    pub async fn has_conversation(&self, conversation_id: &str) -> Result<bool> {
        Ok(session_index::list_sessions(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
        )?
        .into_iter()
        .any(|row| row.conversation_id == conversation_id))
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
        .filter(|row| {
            !should_hide_empty_placeholder_row(
                &row.conversation_id,
                row.message_count,
                row.title.as_deref(),
            )
        })
        .map(|row| StoredConversationSummary {
            conversation_id: row.conversation_id,
            title: row.title,
            message_count: row.message_count,
            updated_at_ms: row.updated_at_ms,
            archived: row.archived,
        })
        .collect())
    }

    pub async fn list_conversations_page(
        &self,
        cursor: Option<String>,
        limit: usize,
    ) -> Result<ConversationListPage> {
        let cursor = cursor
            .as_deref()
            .map(session_index::SessionListCursor::decode)
            .transpose()?;
        let page = session_index::list_sessions_page(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
            cursor,
            limit,
        )?;
        let summaries = page
            .rows
            .into_iter()
            .filter(|row| {
                !should_hide_empty_placeholder_row(
                    &row.conversation_id,
                    row.message_count,
                    row.title.as_deref(),
                )
            })
            .map(|row| StoredConversationSummary {
                conversation_id: row.conversation_id,
                title: row.title,
                message_count: row.message_count,
                updated_at_ms: row.updated_at_ms,
                archived: row.archived,
            })
            .collect();
        Ok(ConversationListPage {
            conversations: stored_summaries_to_protocol(summaries),
            has_more: page.has_more,
            next_cursor: page.next_cursor.map(|cursor| cursor.encode()),
        })
    }

    pub async fn purge_missing_conversation_if_needed(
        &self,
        conversation_id: &str,
    ) -> Result<bool> {
        if self.rollout_exists(conversation_id) {
            return Ok(false);
        }
        let row = session_index::list_sessions(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
        )?
        .into_iter()
        .find(|row| row.conversation_id == conversation_id);
        if row.as_ref().is_none_or(|row| {
            should_hide_empty_placeholder_row(
                &row.conversation_id,
                row.message_count,
                row.title.as_deref(),
            )
        }) {
            return Ok(false);
        }
        session_index::delete_session(&session_index::db_path(&self.root), conversation_id)?;
        Ok(true)
    }

    pub async fn reconcile_missing_conversations(
        &self,
        limit: usize,
    ) -> Result<ConversationReconcileReport> {
        let limit = limit.max(1);
        let rows = session_index::list_sessions(
            &session_index::db_path(&self.root),
            &self.root.to_string_lossy(),
        )?;
        let truncated = rows.len() > limit;
        let mut removed = Vec::new();
        for row in rows.iter().take(limit) {
            if !should_hide_empty_placeholder_row(
                &row.conversation_id,
                row.message_count,
                row.title.as_deref(),
            ) && !self.rollout_exists(&row.conversation_id)
            {
                removed.push(row.conversation_id.clone());
            }
        }
        session_index::delete_sessions(&session_index::db_path(&self.root), &removed)?;
        Ok(ConversationReconcileReport {
            checked: rows.len().min(limit),
            removed,
            truncated,
        })
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
        let canonical = self.canonical_rollout_path(conversation_id);
        let legacy = self.legacy_rollout_path(conversation_id);
        if canonical != legacy && legacy.exists() && !canonical.exists() {
            let _ = std::fs::rename(&legacy, &canonical);
        }
        if canonical.exists() {
            return canonical;
        }
        if legacy.exists() {
            return legacy;
        }
        canonical
    }

    fn rollout_exists(&self, conversation_id: &str) -> bool {
        self.canonical_rollout_path(conversation_id).exists()
            || self.legacy_rollout_path(conversation_id).exists()
    }

    fn canonical_rollout_path(&self, conversation_id: &str) -> PathBuf {
        self.root.join(format!(
            "{}.rollout.jsonl",
            canonicalize_conversation_file_stem(conversation_id)
        ))
    }

    fn legacy_rollout_path(&self, conversation_id: &str) -> PathBuf {
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

    async fn load_rollout_items_page_from_path(
        &self,
        path: &Path,
        conversation_id: &str,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<RolloutItemsPage> {
        if let Some(page) = self
            .load_rollout_items_page_from_index(path, conversation_id, before_turn_id, limit)
            .await?
        {
            return Ok(page);
        }

        self.rebuild_turn_index_from_path(path, conversation_id)
            .await?;
        if let Some(page) = self
            .load_rollout_items_page_from_index(path, conversation_id, before_turn_id, limit)
            .await?
        {
            return Ok(page);
        }

        self.load_rollout_items_page_by_scanning(path, before_turn_id, limit)
            .await
    }

    async fn load_rollout_items_page_from_index(
        &self,
        path: &Path,
        conversation_id: &str,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<Option<RolloutItemsPage>> {
        let file_len = match fs::metadata(path).await {
            Ok(metadata) => metadata.len(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Some(RolloutItemsPage {
                    items: Vec::new(),
                    has_more: false,
                }));
            }
            Err(err) => {
                return Err(err).with_context(|| format!("failed to stat {}", path.display()));
            }
        };
        let db_path = session_index::db_path(&self.root);
        let before_offset = match before_turn_id {
            Some(turn_id) => session_index::turn_start_offset(&db_path, conversation_id, turn_id)?,
            None => None,
        };
        let mut rows = session_index::turn_index_page_before_offset(
            &db_path,
            conversation_id,
            before_offset,
            limit.max(1).saturating_add(1),
        )?;
        if rows.is_empty() {
            return Ok(None);
        }
        let page_limit = limit.max(1);
        let has_more = rows.len() > page_limit;
        if has_more {
            rows.remove(0);
        }
        let start_offset = rows
            .first()
            .map(|row| row.start_offset)
            .unwrap_or(file_len)
            .min(file_len);
        let end_offset = before_offset.unwrap_or(file_len).min(file_len);
        if start_offset >= end_offset {
            return Ok(None);
        }

        let mut file = fs::File::open(path)
            .await
            .with_context(|| format!("failed to read {}", path.display()))?;
        file.seek(SeekFrom::Start(start_offset))
            .await
            .with_context(|| format!("failed to seek {}", path.display()))?;
        let mut bytes = vec![0; (end_offset - start_offset) as usize];
        file.read_exact(&mut bytes)
            .await
            .with_context(|| format!("failed to read {}", path.display()))?;
        let text = String::from_utf8(bytes)
            .with_context(|| format!("rollout slice is not valid UTF-8: {}", path.display()))?;
        let items = self.parse_rollout_log_text(path, &text)?;
        Ok(Some(RolloutItemsPage { items, has_more }))
    }

    async fn rebuild_turn_index_from_path(&self, path: &Path, conversation_id: &str) -> Result<()> {
        let text = match self.read_rollout_log_text(path).await? {
            Some(text) => text,
            None => {
                session_index::replace_turn_index(
                    &session_index::db_path(&self.root),
                    conversation_id,
                    &[],
                )?;
                return Ok(());
            }
        };

        let parsed_lines = rollout_log::parse_lines_with_offsets(path, &text)?;
        let rows = parsed_lines
            .into_iter()
            .filter_map(|line| {
                rollout_turn_start_id(&line.item).map(|turn_id| session_index::TurnIndexRow {
                    turn_id: turn_id.to_string(),
                    start_offset: line.start_offset,
                })
            })
            .collect::<Vec<_>>();

        session_index::replace_turn_index(
            &session_index::db_path(&self.root),
            conversation_id,
            &rows,
        )?;
        Ok(())
    }

    async fn load_rollout_items_page_by_scanning(
        &self,
        path: &Path,
        before_turn_id: Option<&str>,
        limit: usize,
    ) -> Result<RolloutItemsPage> {
        let text = match self.read_rollout_log_text(path).await? {
            Some(text) => text,
            None => {
                return Ok(RolloutItemsPage {
                    items: Vec::new(),
                    has_more: false,
                });
            }
        };

        let page_limit = limit.max(1);
        let parsed_lines = rollout_log::parse_lines_with_offsets(path, &text)?;
        let mut saw_explicit_turn = false;
        let mut legacy_items = Vec::new();
        let mut current_turn: Option<RolloutTurnChunk> = None;
        let mut window: VecDeque<RolloutTurnChunk> = VecDeque::new();

        for line in parsed_lines {
            let item = line.item;
            if let Some(turn_id) = rollout_turn_start_id(&item) {
                if before_turn_id.is_some_and(|before| before == turn_id) {
                    break;
                }
                saw_explicit_turn = true;
                push_turn_page_chunk(&mut window, current_turn.take(), page_limit);
                current_turn = Some(RolloutTurnChunk { items: vec![item] });
                continue;
            }

            if let Some(turn) = current_turn.as_mut() {
                turn.items.push(item);
            } else {
                legacy_items.push(item);
            }
        }

        push_turn_page_chunk(&mut window, current_turn.take(), page_limit);

        if !saw_explicit_turn {
            return Ok(RolloutItemsPage {
                items: legacy_items,
                has_more: false,
            });
        }

        let has_more = window.len() > page_limit;
        if has_more {
            window.pop_front();
        }
        let items = window
            .into_iter()
            .flat_map(|chunk| chunk.items)
            .collect::<Vec<_>>();
        Ok(RolloutItemsPage { items, has_more })
    }

    async fn read_rollout_log_text(&self, path: &Path) -> Result<Option<String>> {
        match fs::read_to_string(path).await {
            Ok(text) => Ok(Some(text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    fn parse_rollout_log_text(&self, path: &Path, text: &str) -> Result<Vec<RolloutItem>> {
        rollout_log::parse_items(path, text)
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
                ResponseItem::User { content } => !input_items_are_blank(content),
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

fn canonicalize_conversation_file_stem(value: &str) -> String {
    if let Some(chat_id) = value.strip_prefix("agent:main:feishu:dm:") {
        return format!("feishu_dm_{}", sanitize_conversation_id(chat_id));
    }
    if let Some(chat_id) = value.strip_prefix("agent:main:feishu:group:") {
        return format!("feishu_group_{}", sanitize_conversation_id(chat_id));
    }
    sanitize_conversation_id(value)
}

fn should_hide_empty_placeholder_row(
    conversation_id: &str,
    message_count: usize,
    title: Option<&str>,
) -> bool {
    is_timestamp_conversation_id(conversation_id)
        && message_count == 0
        && title.is_none_or(|value| value.trim().is_empty())
}

fn is_timestamp_conversation_id(value: &str) -> bool {
    let mut parts = value.split('-');
    let date = parts.next().unwrap_or_default();
    let time = parts.next().unwrap_or_default();
    let suffix = parts.next().unwrap_or_default();
    parts.next().is_none()
        && date.len() == 8
        && time.len() == 6
        && suffix.len() == 4
        && date.chars().all(|c| c.is_ascii_digit())
        && time.chars().all(|c| c.is_ascii_digit())
        && suffix.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ApprovalGrantKey, TurnItemDeltaKind};
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_path(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let counter = TEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("{prefix}-{unique}-{counter}"))
    }

    #[tokio::test]
    async fn concurrent_event_appends_leave_valid_json() {
        let root = unique_temp_path("cloudagent-storage-test");
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
                                user_input: agent_core::text_input_items(format!(
                                    "message-{index}-{item}"
                                )),
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
    async fn load_rollout_items_page_reads_latest_explicit_turn_window() {
        let root = unique_temp_path("cloudagent-storage-page");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "paged-history";

        for index in 1..=5 {
            let turn_id = format!("turn-{index}");
            store
                .append_rollout_items(
                    conversation_id,
                    &[
                        RolloutItem::from(EventMsg::TurnStarted {
                            turn_id: turn_id.clone(),
                            conversation_id: conversation_id.to_string(),
                            user_input: agent_core::text_input_items(format!("message {index}")),
                        }),
                        RolloutItem::from(EventMsg::TurnCompleted { turn_id }),
                    ],
                )
                .await
                .expect("append turn");
        }

        let page = store
            .load_rollout_items_page(conversation_id, None, 2)
            .await
            .expect("load latest page");
        let turns = agent_core::build_turns_from_rollout_items(&page.items);
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-4", "turn-5"]
        );
        assert!(page.has_more);

        let page = store
            .load_rollout_items_page(conversation_id, Some("turn-4"), 2)
            .await
            .expect("load older page");
        let turns = agent_core::build_turns_from_rollout_items(&page.items);
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-2", "turn-3"]
        );
        assert!(page.has_more);

        let page = store
            .load_rollout_items_page(conversation_id, Some("turn-2"), 2)
            .await
            .expect("load oldest page");
        let turns = agent_core::build_turns_from_rollout_items(&page.items);
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-1"]
        );
        assert!(!page.has_more);

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn load_rollout_items_page_rebuilds_missing_turn_index() {
        let root = unique_temp_path("cloudagent-storage-page-rebuild");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "paged-history-rebuild";
        fs::create_dir_all(&root).await.expect("create root");

        let mut lines = Vec::new();
        for index in 1..=4 {
            let turn_id = format!("turn-{index}");
            lines.push(
                serde_json::to_string(&RolloutItem::from(EventMsg::TurnStarted {
                    turn_id: turn_id.clone(),
                    conversation_id: conversation_id.to_string(),
                    user_input: agent_core::text_input_items(format!("message {index}")),
                }))
                .expect("serialize turn start"),
            );
            lines.push(
                serde_json::to_string(&RolloutItem::from(EventMsg::TurnCompleted { turn_id }))
                    .expect("serialize turn completed"),
            );
        }
        fs::write(
            store.rollout_path(conversation_id),
            format!("{}\n", lines.join("\n")),
        )
        .await
        .expect("write rollout");

        let page = store
            .load_rollout_items_page(conversation_id, None, 2)
            .await
            .expect("load latest page");
        let turns = agent_core::build_turns_from_rollout_items(&page.items);
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-3", "turn-4"]
        );
        assert!(page.has_more);

        let indexed_rows = session_index::turn_index_page_before_offset(
            &session_index::db_path(&root),
            conversation_id,
            None,
            10,
        )
        .expect("load rebuilt index");
        assert_eq!(
            indexed_rows
                .iter()
                .map(|row| row.turn_id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-1", "turn-2", "turn-3", "turn-4"]
        );

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn load_rollout_items_ignores_truncated_tail_record() {
        let root = unique_temp_path("cloudagent-storage-truncated-tail");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "truncated-tail";
        fs::create_dir_all(&root).await.expect("create root");

        let complete = serde_json::to_string(&RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: conversation_id.to_string(),
            user_input: agent_core::text_input_items("message 1"),
        }))
        .expect("serialize complete rollout item");
        fs::write(
            store.rollout_path(conversation_id),
            format!("{complete}\n{{\"type\":\"response_item\""),
        )
        .await
        .expect("write truncated rollout");

        let items = store
            .load_rollout_items(conversation_id)
            .await
            .expect("load rollout with truncated tail");

        assert_eq!(items.len(), 1);
        assert!(matches!(
            &items[0],
            RolloutItem::EventMsg {
                event: EventMsg::TurnStarted { turn_id, .. },
            } if turn_id == "turn-1"
        ));

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn load_rollout_items_rejects_malformed_middle_record() {
        let root = unique_temp_path("cloudagent-storage-bad-middle");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "bad-middle";
        fs::create_dir_all(&root).await.expect("create root");

        let first = serde_json::to_string(&RolloutItem::from(EventMsg::TurnStarted {
            turn_id: "turn-1".to_string(),
            conversation_id: conversation_id.to_string(),
            user_input: agent_core::text_input_items("message 1"),
        }))
        .expect("serialize first item");
        let second = serde_json::to_string(&RolloutItem::from(EventMsg::TurnCompleted {
            turn_id: "turn-1".to_string(),
        }))
        .expect("serialize second item");
        fs::write(
            store.rollout_path(conversation_id),
            format!("{first}\n{{\"type\":\"response_item\"\n{second}\n"),
        )
        .await
        .expect("write malformed middle rollout");

        let err = store
            .load_rollout_items(conversation_id)
            .await
            .expect_err("middle corruption should fail");

        assert!(
            err.to_string().contains("failed to parse rollout file"),
            "unexpected error: {err:?}"
        );

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn load_rollout_items_page_rebuild_ignores_truncated_tail_record() {
        let root = unique_temp_path("cloudagent-storage-page-truncated-tail");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "paged-truncated-tail";
        fs::create_dir_all(&root).await.expect("create root");

        let mut lines = Vec::new();
        for index in 1..=3 {
            let turn_id = format!("turn-{index}");
            lines.push(
                serde_json::to_string(&RolloutItem::from(EventMsg::TurnStarted {
                    turn_id: turn_id.clone(),
                    conversation_id: conversation_id.to_string(),
                    user_input: agent_core::text_input_items(format!("message {index}")),
                }))
                .expect("serialize turn start"),
            );
            lines.push(
                serde_json::to_string(&RolloutItem::from(EventMsg::TurnCompleted { turn_id }))
                    .expect("serialize turn completed"),
            );
        }
        fs::write(
            store.rollout_path(conversation_id),
            format!("{}\n{{\"type\":\"event_msg\"", lines.join("\n")),
        )
        .await
        .expect("write truncated page rollout");

        let page = store
            .load_rollout_items_page(conversation_id, None, 2)
            .await
            .expect("load latest page with truncated tail");
        let turns = agent_core::build_turns_from_rollout_items(&page.items);
        assert_eq!(
            turns
                .iter()
                .map(|turn| turn.id.as_str())
                .collect::<Vec<_>>(),
            vec!["turn-2", "turn-3"]
        );
        assert!(page.has_more);

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn direct_append_events_filters_streaming_deltas() {
        let root = unique_temp_path("cloudagent-storage-policy");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "streaming-deltas";

        store
            .append_event(
                conversation_id,
                &EventMsg::ItemDelta {
                    turn_id: "turn-1".to_string(),
                    item_id: "assistant:1".to_string(),
                    call_id: None,
                    kind: TurnItemDeltaKind::Text,
                    segment_index: None,
                    delta: "hello".to_string(),
                },
            )
            .await
            .expect("append filtered delta");

        assert!(
            !store.rollout_path(conversation_id).exists(),
            "filtered streaming deltas should not materialize rollout files"
        );

        let _ = fs::remove_dir_all(root).await;
    }

    #[test]
    fn canonicalize_conversation_file_stem_shortens_feishu_private_sessions() {
        assert_eq!(
            canonicalize_conversation_file_stem("agent:main:feishu:dm:oc_123"),
            "feishu_dm_oc_123"
        );
    }

    #[tokio::test]
    async fn rollout_path_migrates_legacy_feishu_file_name() {
        let root = unique_temp_path("cloudagent-storage-test");
        fs::create_dir_all(&root).await.expect("create root");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "agent:main:feishu:dm:oc_legacy";
        let legacy = root.join("agent_main_feishu_dm_oc_legacy.rollout.jsonl");
        fs::write(&legacy, "").await.expect("write legacy file");

        let resolved = store.rollout_path(conversation_id);

        assert_eq!(
            resolved.file_name().and_then(|name| name.to_str()),
            Some("feishu_dm_oc_legacy.rollout.jsonl")
        );
        assert!(resolved.exists());
        assert!(!legacy.exists());

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn pruning_removes_oldest_archived_conversations_first() {
        let root = unique_temp_path("cloudagent-storage-prune");
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
        let root = unique_temp_path("cloudagent-approval-grants");
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

    #[tokio::test]
    async fn marking_active_conversation_does_not_create_empty_session() {
        let root = unique_temp_path("cloudagent-placeholder-session");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "20260531-181152-ab3f";
        store.ensure_root_dir().await.expect("create root");

        store
            .mark_active_conversation(conversation_id)
            .await
            .expect("mark active conversation");

        assert_eq!(
            store
                .load_active_conversation()
                .await
                .expect("load active conversation"),
            Some(conversation_id.to_string())
        );
        assert!(
            !store
                .has_conversation(conversation_id)
                .await
                .expect("check placeholder visibility")
        );
        assert!(
            store
                .list_conversations()
                .await
                .expect("list conversations")
                .is_empty(),
            "empty placeholder conversations should not appear in the session list before first message"
        );

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn create_conversation_promotes_existing_placeholder_to_visible_session() {
        let root = unique_temp_path("cloudagent-placeholder-promote");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "20260531-181152-b4c2";
        store.ensure_root_dir().await.expect("create root");

        store
            .mark_active_conversation(conversation_id)
            .await
            .expect("mark active conversation");
        store
            .create_conversation(conversation_id)
            .await
            .expect("create conversation");
        store
            .append_rollout_items(
                conversation_id,
                &[RolloutItem::from(ResponseItem::User {
                    content: agent_core::text_input_items("hello"),
                })],
            )
            .await
            .expect("append first message");

        assert!(
            store
                .has_conversation(conversation_id)
                .await
                .expect("check promoted conversation")
        );
        let summaries = store
            .list_conversations()
            .await
            .expect("list conversations");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].conversation_id, conversation_id);

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn empty_persisted_placeholder_rows_are_hidden_from_session_list() {
        let root = unique_temp_path("cloudagent-hidden-placeholder");
        let store = JsonConversationStore::new(&root);
        store.ensure_root_dir().await.expect("create root");

        session_index::upsert_session(
            &session_index::db_path(&root),
            &root.to_string_lossy(),
            "20260531-181152-c3d4",
            0,
            now_ms(),
            false,
            None,
        )
        .expect("upsert placeholder row");

        let summaries = store
            .list_conversations()
            .await
            .expect("list conversations");
        assert!(summaries.is_empty());

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn archived_conversation_with_fallback_placeholder_keeps_session_list_clean() {
        let root = unique_temp_path("cloudagent-archive-fallback");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "session-archive-me";
        let fallback_placeholder_id = "20260531-181152-d4e5";
        store.ensure_root_dir().await.expect("create root");

        store
            .create_conversation(conversation_id)
            .await
            .expect("create conversation");
        store
            .archive_conversation(conversation_id)
            .await
            .expect("archive conversation");
        store
            .mark_active_conversation(fallback_placeholder_id)
            .await
            .expect("mark fallback placeholder active");

        assert_eq!(
            store
                .load_active_conversation()
                .await
                .expect("load active conversation"),
            Some(fallback_placeholder_id.to_string())
        );
        assert!(
            store
                .list_conversations()
                .await
                .expect("list conversations")
                .is_empty(),
            "archived conversations and empty fallback placeholders should stay out of /session"
        );

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn deleted_conversation_with_fallback_placeholder_keeps_session_list_clean() {
        let root = unique_temp_path("cloudagent-delete-fallback");
        let store = JsonConversationStore::new(&root);
        let conversation_id = "session-delete-me";
        let fallback_placeholder_id = "20260531-181152-e5f6";
        store.ensure_root_dir().await.expect("create root");

        store
            .create_conversation(conversation_id)
            .await
            .expect("create conversation");
        store
            .delete_conversation(conversation_id)
            .await
            .expect("delete conversation");
        store
            .mark_active_conversation(fallback_placeholder_id)
            .await
            .expect("mark fallback placeholder active");

        assert!(
            !store
                .has_conversation(conversation_id)
                .await
                .expect("conversation deleted"),
            "deleted conversations should be removed from the formal session store"
        );
        assert_eq!(
            store
                .load_active_conversation()
                .await
                .expect("load active conversation"),
            Some(fallback_placeholder_id.to_string())
        );
        assert!(
            store
                .list_conversations()
                .await
                .expect("list conversations")
                .is_empty(),
            "deleted conversations and empty fallback placeholders should stay out of /session"
        );

        let _ = fs::remove_dir_all(root).await;
    }

    #[test]
    fn timestamp_conversation_ids_are_detected() {
        assert!(is_timestamp_conversation_id("20260531-181152-ab3f"));
        assert!(is_timestamp_conversation_id("20260531-181152-0000"));
        assert!(!is_timestamp_conversation_id("20260531-181152"));
        assert!(!is_timestamp_conversation_id("20260531-181152-ab3f-extra"));
    }
}

#[cfg(test)]
#[path = "session_list_optimization_tests.rs"]
mod session_list_optimization_tests;
