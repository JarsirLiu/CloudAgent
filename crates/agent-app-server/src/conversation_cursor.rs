use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CURSOR_DIR: &str = ".cloudagent";
const CURSOR_FILE: &str = "last_active_conversation.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConversationCursor {
    conversation_id: String,
}

fn cursor_path(project_root: &Path) -> PathBuf {
    project_root.join(CURSOR_DIR).join(CURSOR_FILE)
}

pub(crate) fn load(project_root: &Path) -> Option<String> {
    let path = cursor_path(project_root);
    let text = fs::read_to_string(path).ok()?;
    let cursor: ConversationCursor = serde_json::from_str(&text).ok()?;
    Some(cursor.conversation_id)
}

pub(crate) fn save(project_root: &Path, conversation_id: &str) -> Result<()> {
    let path = cursor_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = ConversationCursor {
        conversation_id: conversation_id.to_string(),
    };
    fs::write(path, serde_json::to_vec_pretty(&payload)?)?;
    Ok(())
}

