use agent_core::session::AgentSession;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Clone, Debug)]
pub struct JsonSessionStore {
    root: PathBuf,
}

impl JsonSessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
        fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let path = self.session_path(&session.id);
        let text = serde_json::to_string_pretty(session)?;
        fs::write(&path, text)
            .await
            .with_context(|| format!("failed to write {}", path.display()))?;
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

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.root.join(format!("{}.json", sanitize_session_id(session_id)))
    }
}

fn sanitize_session_id(value: &str) -> String {
    value.chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}
