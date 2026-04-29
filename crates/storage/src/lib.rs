use agent_core::session::AgentSession;
use agent_protocol::TurnEvent;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct JsonSessionStore {
    root: PathBuf,
    io_lock: Arc<Mutex<()>>,
}

impl JsonSessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn load_session(&self, session_id: &str) -> Result<Option<AgentSession>> {
        let path = self.session_path(session_id);
        match fs::read_to_string(&path).await {
            Ok(text) => {
                let session = serde_json::from_str::<AgentSession>(&text)
                    .with_context(|| format!("failed to parse session file {}", path.display()))?;
                Ok(Some(session))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    pub async fn save_session(&self, session: &AgentSession) -> Result<()> {
        let _guard = self.io_lock.lock().await;
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let path = self.session_path(&session.id);
        let text = serde_json::to_string_pretty(session)?;
        self.write_text_atomically(&path, &text).await?;
        Ok(())
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let path = self.session_path(session_id);
        match fs::remove_file(&path).await {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("failed to delete {}", path.display())),
        }
    }

    pub async fn load_events(&self, session_id: &str) -> Result<Vec<TurnEvent>> {
        let path = self.event_path(session_id);
        self.load_events_from_path(&path).await
    }

    pub async fn append_events(&self, session_id: &str, events: &[TurnEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let _guard = self.io_lock.lock().await;
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let path = self.event_path(session_id);
        if let Some(existing) = self.read_event_log_text(&path).await? {
            if existing.trim_start().starts_with('[') {
                let mut all_events = self.parse_event_log_text(&path, &existing)?;
                all_events.extend_from_slice(events);
                let text = self.render_event_log_lines(&all_events)?;
                self.write_text_atomically(&path, &text).await?;
                return Ok(());
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("failed to open {}", path.display()))?;
        for event in events {
            let line = serde_json::to_string(event)?;
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

    pub async fn append_event(&self, session_id: &str, event: &TurnEvent) -> Result<()> {
        self.append_events(session_id, std::slice::from_ref(event)).await
    }

    pub async fn delete_events(&self, session_id: &str) -> Result<()> {
        let path = self.event_path(session_id);
        match fs::remove_file(&path).await {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("failed to delete {}", path.display())),
        }
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join(format!("{}.json", sanitize_session_id(session_id)))
    }

    fn event_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join(format!("{}.events.json", sanitize_session_id(session_id)))
    }

    async fn load_events_from_path(&self, path: &Path) -> Result<Vec<TurnEvent>> {
        match self.read_event_log_text(path).await? {
            Some(text) => self.parse_event_log_text(path, &text),
            None => Ok(Vec::new()),
        }
    }

    async fn read_event_log_text(&self, path: &Path) -> Result<Option<String>> {
        match fs::read_to_string(path).await {
            Ok(text) => Ok(Some(text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    fn parse_event_log_text(&self, path: &Path, text: &str) -> Result<Vec<TurnEvent>> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        if trimmed.starts_with('[') {
            let events = serde_json::from_str::<Vec<TurnEvent>>(trimmed)
                .with_context(|| format!("failed to parse event file {}", path.display()))?;
            return Ok(events);
        }

        let mut events = Vec::new();
        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event = serde_json::from_str::<TurnEvent>(line).with_context(|| {
                format!(
                    "failed to parse event file {} at line {}",
                    path.display(),
                    line_no + 1
                )
            })?;
            events.push(event);
        }
        Ok(events)
    }

    fn render_event_log_lines(&self, events: &[TurnEvent]) -> Result<String> {
        let mut text = String::new();
        for event in events {
            text.push_str(&serde_json::to_string(event)?);
            text.push('\n');
        }
        Ok(text)
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
        fs::rename(&temp_path, path)
            .await
            .with_context(|| format!("failed to rename {} to {}", temp_path.display(), path.display()))?;
        Ok(())
    }
}

fn sanitize_session_id(value: &str) -> String {
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn concurrent_event_appends_leave_valid_json() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cloudagent-storage-test-{unique}"));
        let store = JsonSessionStore::new(&root);
        let session_id = "concurrent-events";

        let mut tasks = Vec::new();
        for index in 0..8usize {
            let cloned = store.clone();
            tasks.push(tokio::spawn(async move {
                for item in 0..10usize {
                    cloned
                        .append_event(
                            session_id,
                            &TurnEvent::TurnStarted {
                                turn_id: format!("turn-{index}-{item}"),
                                session_id: session_id.to_string(),
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

        let events = store.load_events(session_id).await.expect("load events");
        assert_eq!(events.len(), 80);

        let _ = fs::remove_dir_all(root).await;
    }
}
