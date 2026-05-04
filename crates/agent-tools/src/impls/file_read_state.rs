use agent_core::{ReadFileStatus, ResponseItem, RolloutItem, StructuredToolResult};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileReadSnapshot {
    pub(crate) version_token: Option<String>,
    pub(crate) is_partial_view: bool,
}

#[derive(Default)]
struct FileReadState {
    conversations: HashMap<String, HashMap<String, FileReadSnapshot>>,
    restored_conversations: HashSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct FileReadStateStore {
    inner: Arc<Mutex<FileReadState>>,
}

impl FileReadStateStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn record_snapshot(
        &self,
        conversation_id: &str,
        path: &Path,
        version_token: Option<String>,
        is_partial_view: bool,
    ) {
        let path_key = canonical_path_key(path);
        let mut guard = self.inner.lock().await;
        let conversation = guard
            .conversations
            .entry(conversation_id.to_string())
            .or_default();
        if version_token.is_none()
            && conversation
                .get(&path_key)
                .is_some_and(|snapshot| snapshot.version_token.is_some())
        {
            return;
        }
        conversation.insert(
            path_key,
            FileReadSnapshot {
                version_token,
                is_partial_view,
            },
        );
    }

    pub(crate) async fn record_full(
        &self,
        conversation_id: &str,
        path: &Path,
        version_token: String,
    ) {
        self.record_snapshot(conversation_id, path, Some(version_token), false)
            .await;
    }

    pub(crate) async fn get(&self, conversation_id: &str, path: &Path) -> Option<FileReadSnapshot> {
        let path_key = canonical_path_key(path);
        self.inner
            .lock()
            .await
            .conversations
            .get(conversation_id)
            .and_then(|conversation| conversation.get(&path_key).cloned())
    }

    pub(crate) async fn get_or_restore(
        &self,
        conversation_id: &str,
        workspace_root: &Path,
        conversation_store_dir: &Path,
        path: &Path,
    ) -> Result<Option<FileReadSnapshot>> {
        if let Some(snapshot) = self.get(conversation_id, path).await {
            return Ok(Some(snapshot));
        }
        self.restore_from_rollout(conversation_id, workspace_root, conversation_store_dir)
            .await?;
        Ok(self.get(conversation_id, path).await)
    }

    async fn restore_from_rollout(
        &self,
        conversation_id: &str,
        workspace_root: &Path,
        conversation_store_dir: &Path,
    ) -> Result<()> {
        {
            let guard = self.inner.lock().await;
            if guard.restored_conversations.contains(conversation_id) {
                return Ok(());
            }
        }

        let rollout_path = conversation_store_dir.join(format!(
            "{}.rollout.jsonl",
            sanitize_conversation_id(conversation_id)
        ));
        let text = match tokio::fs::read_to_string(&rollout_path).await {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let mut guard = self.inner.lock().await;
                guard
                    .restored_conversations
                    .insert(conversation_id.to_string());
                return Ok(());
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to read rollout log {}", rollout_path.display())
                });
            }
        };

        let mut restored = HashMap::new();
        for (line_no, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let item = serde_json::from_str::<RolloutItem>(trimmed).with_context(|| {
                format!(
                    "failed to parse rollout file {} at line {}",
                    rollout_path.display(),
                    line_no + 1
                )
            })?;
            apply_rollout_item_to_state(&mut restored, workspace_root, &item);
        }

        let mut guard = self.inner.lock().await;
        let conversation = guard
            .conversations
            .entry(conversation_id.to_string())
            .or_default();
        for (path_key, snapshot) in restored {
            if snapshot.version_token.is_none()
                && conversation
                    .get(&path_key)
                    .is_some_and(|existing| existing.version_token.is_some())
            {
                continue;
            }
            conversation.insert(path_key, snapshot);
        }
        guard
            .restored_conversations
            .insert(conversation_id.to_string());
        Ok(())
    }
}

fn apply_rollout_item_to_state(
    restored: &mut HashMap<String, FileReadSnapshot>,
    workspace_root: &Path,
    item: &RolloutItem,
) {
    let RolloutItem::ResponseItem {
        item: ResponseItem::Tool { structured, .. },
    } = item
    else {
        return;
    };

    match structured {
        Some(StructuredToolResult::ReadFile { read, .. }) => {
            if read.status != ReadFileStatus::Ok {
                return;
            }
            let path_key = canonical_rollout_path_key(workspace_root, &read.path);
            let is_partial_view = read.truncated || read.start_line.unwrap_or(1) > 1;
            if let Some(version_token) = &read.version_token {
                restored.insert(
                    path_key,
                    FileReadSnapshot {
                        version_token: Some(version_token.clone()),
                        is_partial_view,
                    },
                );
            } else if !restored
                .get(&path_key)
                .is_some_and(|snapshot| snapshot.version_token.is_some())
            {
                restored.insert(
                    path_key,
                    FileReadSnapshot {
                        version_token: None,
                        is_partial_view: true,
                    },
                );
            }
        }
        Some(StructuredToolResult::EditFile {
            changed_paths,
            version_token: Some(version_token),
            ..
        }) if changed_paths.len() == 1 => {
            let path_key = canonical_rollout_path_key(workspace_root, &changed_paths[0]);
            restored.insert(
                path_key,
                FileReadSnapshot {
                    version_token: Some(version_token.clone()),
                    is_partial_view: false,
                },
            );
        }
        _ => {}
    }
}

fn canonical_rollout_path_key(workspace_root: &Path, raw_path: &str) -> String {
    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        canonical_path_key(candidate)
    } else {
        canonical_path_key(&workspace_root.join(candidate))
    }
}

fn canonical_path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
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
    use agent_core::WriteFileStatus;
    use std::path::PathBuf;

    #[tokio::test]
    async fn restore_prefers_full_read_over_later_partial() {
        let store_dir = temp_dir("restore_prefers_full_read_over_later_partial");
        let conversation_id = "test";
        let target_path = store_dir.join("src/lib.rs");
        std::fs::create_dir_all(target_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&target_path, "fn main() {}\n").expect("write file");

        let rollout_path = store_dir.join(format!("{}.rollout.jsonl", conversation_id));
        let items = vec![
            RolloutItem::ResponseItem {
                item: ResponseItem::Tool {
                    tool_call_id: "call-1".to_string(),
                    name: "read_file".to_string(),
                    content: "ok".to_string(),
                    structured: Some(StructuredToolResult::ReadFile {
                        path: target_path.display().to_string(),
                        start_line: Some(1),
                        max_lines: None,
                        total_chars: 10,
                        read: agent_core::ReadFileEntry {
                            path: target_path.display().to_string(),
                            start_line: Some(1),
                            end_line: Some(1),
                            next_start_line: None,
                            returned_line_count: 1,
                            total_line_count: Some(1),
                            returned_char_count: 10,
                            truncated: false,
                            char_count: 10,
                            status: ReadFileStatus::Ok,
                            version_token: Some("abc123".to_string()),
                        },
                    }),
                },
            },
            RolloutItem::ResponseItem {
                item: ResponseItem::Tool {
                    tool_call_id: "call-2".to_string(),
                    name: "read_file".to_string(),
                    content: "partial".to_string(),
                    structured: Some(StructuredToolResult::ReadFile {
                        path: target_path.display().to_string(),
                        start_line: Some(10),
                        max_lines: Some(10),
                        total_chars: 10,
                        read: agent_core::ReadFileEntry {
                            path: target_path.display().to_string(),
                            start_line: Some(10),
                            end_line: Some(10),
                            next_start_line: Some(11),
                            returned_line_count: 1,
                            total_line_count: Some(10),
                            returned_char_count: 10,
                            truncated: true,
                            char_count: 10,
                            status: ReadFileStatus::Ok,
                            version_token: None,
                        },
                    }),
                },
            },
        ];
        let rollout_text = items
            .into_iter()
            .map(|item| serde_json::to_string(&item).expect("serialize"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&rollout_path, rollout_text).expect("write rollout");

        let store = FileReadStateStore::new();
        let snapshot = store
            .get_or_restore(conversation_id, &store_dir, &store_dir, &target_path)
            .await
            .expect("restore")
            .expect("snapshot");

        assert_eq!(
            snapshot,
            FileReadSnapshot {
                version_token: Some("abc123".to_string()),
                is_partial_view: false,
            }
        );
    }

    #[tokio::test]
    async fn restore_uses_latest_edit_version_token() {
        let store_dir = temp_dir("restore_uses_latest_edit_version_token");
        let conversation_id = "test";
        let target_path = store_dir.join("src/lib.rs");
        std::fs::create_dir_all(target_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&target_path, "fn main() {}\n").expect("write file");

        let rollout_path = store_dir.join(format!("{}.rollout.jsonl", conversation_id));
        let items = vec![RolloutItem::ResponseItem {
            item: ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "edit_file".to_string(),
                content: "Edited file".to_string(),
                structured: Some(StructuredToolResult::EditFile {
                    changed_paths: vec![target_path.display().to_string()],
                    files_changed: 1,
                    status: WriteFileStatus::Completed,
                    version_token: Some("def456".to_string()),
                }),
            },
        }];
        let rollout_text = items
            .into_iter()
            .map(|item| serde_json::to_string(&item).expect("serialize"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&rollout_path, rollout_text).expect("write rollout");

        let store = FileReadStateStore::new();
        let snapshot = store
            .get_or_restore(conversation_id, &store_dir, &store_dir, &target_path)
            .await
            .expect("restore")
            .expect("snapshot");

        assert_eq!(
            snapshot,
            FileReadSnapshot {
                version_token: Some("def456".to_string()),
                is_partial_view: false,
            }
        );
    }

    #[tokio::test]
    async fn restore_keeps_version_token_for_partial_read() {
        let store_dir = temp_dir("restore_keeps_version_token_for_partial_read");
        let conversation_id = "test";
        let target_path = store_dir.join("src/lib.rs");
        std::fs::create_dir_all(target_path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&target_path, "line1\nline2\nline3\n").expect("write file");

        let rollout_path = store_dir.join(format!("{}.rollout.jsonl", conversation_id));
        let items = vec![RolloutItem::ResponseItem {
            item: ResponseItem::Tool {
                tool_call_id: "call-1".to_string(),
                name: "read_file".to_string(),
                content: "partial".to_string(),
                structured: Some(StructuredToolResult::ReadFile {
                    path: target_path.display().to_string(),
                    start_line: Some(2),
                    max_lines: Some(1),
                    total_chars: 6,
                    read: agent_core::ReadFileEntry {
                        path: target_path.display().to_string(),
                        start_line: Some(2),
                        end_line: Some(2),
                        next_start_line: None,
                        returned_line_count: 1,
                        total_line_count: Some(3),
                        returned_char_count: 6,
                        truncated: false,
                        char_count: 6,
                        status: ReadFileStatus::Ok,
                        version_token: Some("abc123".to_string()),
                    },
                }),
            },
        }];
        let rollout_text = items
            .into_iter()
            .map(|item| serde_json::to_string(&item).expect("serialize"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&rollout_path, rollout_text).expect("write rollout");

        let store = FileReadStateStore::new();
        let snapshot = store
            .get_or_restore(conversation_id, &store_dir, &store_dir, &target_path)
            .await
            .expect("restore")
            .expect("snapshot");

        assert_eq!(
            snapshot,
            FileReadSnapshot {
                version_token: Some("abc123".to_string()),
                is_partial_view: true,
            }
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudagent_{name}_{unique}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}
