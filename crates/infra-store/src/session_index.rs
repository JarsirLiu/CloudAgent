use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::Path;

const SCHEMA_V1: i32 = 1;
const SCHEMA_V2: i32 = 2;
const SCHEMA_V3: i32 = 3;
const SCHEMA_V4: i32 = 4;
const LATEST_SCHEMA_VERSION: i32 = SCHEMA_V4;

#[derive(Clone, Debug)]
pub struct SessionIndexRow {
    pub conversation_id: String,
    pub title: Option<String>,
    pub message_count: usize,
    pub updated_at_ms: u64,
    pub archived: bool,
}

fn open(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 3000)?;
    migrate_schema(&conn)?;
    Ok(conn)
}

fn migrate_schema(conn: &Connection) -> Result<()> {
    let mut current_version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current_version == 0 {
        conn.execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS sessions(
  conversation_id TEXT PRIMARY KEY,
  project_root TEXT NOT NULL,
  message_count INTEGER NOT NULL DEFAULT 0,
  updated_at_ms INTEGER NOT NULL,
  last_active_at_ms INTEGER NOT NULL,
  archived INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS project_active_session(
  project_root TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS session_events(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id TEXT NOT NULL,
  project_root TEXT NOT NULL,
  event_type TEXT NOT NULL,
  reason TEXT,
  actor TEXT NOT NULL,
  request_id TEXT,
  event_seq INTEGER,
  payload_json TEXT,
  created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_project_updated ON sessions(project_root, updated_at_ms DESC);
CREATE INDEX IF NOT EXISTS idx_events_conversation_time ON session_events(conversation_id, created_at_ms DESC);
"#,
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_V1)?;
        current_version = SCHEMA_V1;
    }

    if current_version < SCHEMA_V2 {
        conn.execute("ALTER TABLE sessions ADD COLUMN title TEXT", [])?;
        conn.pragma_update(None, "user_version", SCHEMA_V2)?;
        current_version = SCHEMA_V2;
    }

    if current_version < SCHEMA_V3 {
        conn.execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS project_settings(
  project_root TEXT PRIMARY KEY,
  config_json TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
"#,
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_V3)?;
        current_version = SCHEMA_V3;
    }

    if current_version < SCHEMA_V4 {
        conn.execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS approval_grants(
  conversation_id TEXT NOT NULL,
  grant_key_json TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  PRIMARY KEY(conversation_id, grant_key_json)
);
CREATE INDEX IF NOT EXISTS idx_approval_grants_conversation
  ON approval_grants(conversation_id);
"#,
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_V4)?;
        current_version = SCHEMA_V4;
    }

    if current_version > LATEST_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported session_index schema version {current_version}, max supported is {LATEST_SCHEMA_VERSION}"
        );
    }
    Ok(())
}

pub fn upsert_project_settings(
    db_path: &Path,
    project_root: &str,
    config_json: &str,
    updated_at_ms: u64,
) -> Result<()> {
    let conn = open(db_path)?;
    conn.execute(
        r#"INSERT INTO project_settings(project_root, config_json, updated_at_ms)
VALUES(?1, ?2, ?3)
ON CONFLICT(project_root) DO UPDATE SET config_json=excluded.config_json, updated_at_ms=excluded.updated_at_ms"#,
        params![project_root, config_json, updated_at_ms as i64],
    )?;
    Ok(())
}

pub fn get_project_settings(db_path: &Path, project_root: &str) -> Result<Option<String>> {
    let conn = open(db_path)?;
    let mut stmt =
        conn.prepare("SELECT config_json FROM project_settings WHERE project_root=?1")?;
    let mut rows = stmt.query(params![project_root])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn db_path(root: &Path) -> std::path::PathBuf {
    root.join("session_index.db")
}

pub fn upsert_session(
    db_path: &Path,
    project_root: &str,
    conversation_id: &str,
    message_count: usize,
    updated_at_ms: u64,
    archived: bool,
    title: Option<&str>,
) -> Result<()> {
    let conn = open(db_path)?;
    conn.execute(
        r#"INSERT INTO sessions(conversation_id, project_root, message_count, updated_at_ms, last_active_at_ms, archived, title)
VALUES(?1, ?2, ?3, ?4, ?4, ?5, ?6)
ON CONFLICT(conversation_id) DO UPDATE SET
  project_root=excluded.project_root,
  message_count=excluded.message_count,
  updated_at_ms=excluded.updated_at_ms,
  archived=excluded.archived,
  title=COALESCE(excluded.title, sessions.title)"#,
        params![conversation_id, project_root, message_count as i64, updated_at_ms as i64, if archived {1} else {0}, title],
    )?;
    Ok(())
}

pub fn mark_active(
    db_path: &Path,
    project_root: &str,
    conversation_id: &str,
    updated_at_ms: u64,
) -> Result<()> {
    let conn = open(db_path)?;
    conn.execute(
        r#"INSERT INTO project_active_session(project_root, conversation_id, updated_at_ms)
VALUES(?1, ?2, ?3)
ON CONFLICT(project_root) DO UPDATE SET conversation_id=excluded.conversation_id, updated_at_ms=excluded.updated_at_ms"#,
        params![project_root, conversation_id, updated_at_ms as i64],
    )?;
    conn.execute(
        "UPDATE sessions SET last_active_at_ms=?2 WHERE conversation_id=?1",
        params![conversation_id, updated_at_ms as i64],
    )?;
    Ok(())
}

pub fn get_active(db_path: &Path, project_root: &str) -> Result<Option<String>> {
    let conn = open(db_path)?;
    let mut stmt =
        conn.prepare("SELECT conversation_id FROM project_active_session WHERE project_root=?1")?;
    let mut rows = stmt.query(params![project_root])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn list_sessions(db_path: &Path, project_root: &str) -> Result<Vec<SessionIndexRow>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT conversation_id, title, message_count, updated_at_ms, archived FROM sessions WHERE project_root=?1 AND archived=0 ORDER BY updated_at_ms DESC",
    )?;
    let mut rows = stmt.query(params![project_root])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(SessionIndexRow {
            conversation_id: row.get(0)?,
            title: row.get(1)?,
            message_count: row.get::<_, i64>(2)? as usize,
            updated_at_ms: row.get::<_, i64>(3)? as u64,
            archived: row.get::<_, i64>(4)? != 0,
        });
    }
    Ok(out)
}

pub fn list_archived_sessions(db_path: &Path, project_root: &str) -> Result<Vec<SessionIndexRow>> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT conversation_id, title, message_count, updated_at_ms, archived FROM sessions WHERE project_root=?1 AND archived=1 ORDER BY updated_at_ms ASC",
    )?;
    let mut rows = stmt.query(params![project_root])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(SessionIndexRow {
            conversation_id: row.get(0)?,
            title: row.get(1)?,
            message_count: row.get::<_, i64>(2)? as usize,
            updated_at_ms: row.get::<_, i64>(3)? as u64,
            archived: row.get::<_, i64>(4)? != 0,
        });
    }
    Ok(out)
}

pub fn set_title(db_path: &Path, conversation_id: &str, title: &str) -> Result<()> {
    let conn = open(db_path)?;
    conn.execute(
        "UPDATE sessions SET title=?2 WHERE conversation_id=?1",
        params![conversation_id, title],
    )?;
    Ok(())
}

pub fn delete_session(db_path: &Path, conversation_id: &str) -> Result<()> {
    let conn = open(db_path)?;
    conn.execute(
        "DELETE FROM sessions WHERE conversation_id=?1",
        params![conversation_id],
    )?;
    conn.execute(
        "DELETE FROM project_active_session WHERE conversation_id=?1",
        params![conversation_id],
    )?;
    conn.execute(
        "DELETE FROM session_events WHERE conversation_id=?1",
        params![conversation_id],
    )?;
    conn.execute(
        "DELETE FROM approval_grants WHERE conversation_id=?1",
        params![conversation_id],
    )?;
    Ok(())
}

pub fn has_approval_grant(
    db_path: &Path,
    conversation_id: &str,
    grant_key_json: &str,
) -> Result<bool> {
    let conn = open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT 1 FROM approval_grants WHERE conversation_id=?1 AND grant_key_json=?2 LIMIT 1",
    )?;
    let mut rows = stmt.query(params![conversation_id, grant_key_json])?;
    Ok(rows.next()?.is_some())
}

pub fn upsert_approval_grant(
    db_path: &Path,
    conversation_id: &str,
    grant_key_json: &str,
    created_at_ms: u64,
) -> Result<()> {
    let conn = open(db_path)?;
    conn.execute(
        r#"INSERT INTO approval_grants(conversation_id, grant_key_json, created_at_ms)
VALUES(?1, ?2, ?3)
ON CONFLICT(conversation_id, grant_key_json) DO UPDATE SET created_at_ms=excluded.created_at_ms"#,
        params![conversation_id, grant_key_json, created_at_ms as i64],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn append_event(
    db_path: &Path,
    project_root: &str,
    conversation_id: &str,
    event_type: &str,
    reason: Option<&str>,
    actor: &str,
    request_id: Option<&str>,
    event_seq: Option<i64>,
    payload_json: Option<&str>,
    created_at_ms: u64,
) -> Result<()> {
    let conn = open(db_path)?;
    conn.execute(
        "INSERT INTO session_events(conversation_id, project_root, event_type, reason, actor, request_id, event_seq, payload_json, created_at_ms) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![conversation_id, project_root, event_type, reason, actor, request_id, event_seq, payload_json, created_at_ms as i64],
    )?;
    Ok(())
}
