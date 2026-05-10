use anyhow::Result;
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{OpenOptions, create_dir_all};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Serialize)]
pub struct ContextBudgetLogEntry {
    pub conversation_id: String,
    pub turn_id: String,
    pub model_context_window: u64,
    pub trigger_ratio: f32,
    pub trigger_tokens: usize,
    pub estimated_total_tokens: usize,
    pub filter_enabled: bool,
    pub sdk_total_tokens: Option<usize>,
    pub history_tokens: usize,
    pub overhead_tokens: usize,
    pub memory_floor_tokens: usize,
    pub safety_buffer_tokens: usize,
    pub compaction_triggered: bool,
    pub hard_cap_triggered: bool,
    pub memory_before: usize,
    pub memory_after: usize,
    pub skills_before: usize,
    pub skills_after: usize,
    pub mcp_before: usize,
    pub mcp_after: usize,
}

pub fn append_context_budget_log(
    data_root_dir: &Path,
    entry: &ContextBudgetLogEntry,
) -> Result<()> {
    let dir = data_root_dir.join("logs");
    create_dir_all(&dir)?;
    let file = dir.join("context_budget.jsonl");
    let mut handle = OpenOptions::new().create(true).append(true).open(file)?;
    let line = serde_json::to_string(entry)?;
    handle.write_all(line.as_bytes())?;
    handle.write_all(b"\n")?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct AuditEventEntry<'a> {
    pub session_id: &'a str,
    pub turn_id: Option<&'a str>,
    pub event_type: &'a str,
    pub severity: &'a str,
    pub payload_json: String,
}

const AUDIT_SCHEMA_VERSION: i64 = 1;
const AUDIT_RETENTION_DAYS: i64 = 30;
static AUDIT_WRITE_FAILURES: AtomicU64 = AtomicU64::new(0);
static AUDIT_LAST_ERROR_LOG_MS: AtomicU64 = AtomicU64::new(0);
static RETENTION_LAST_RUN_MS: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

pub fn append_audit_event_safe(data_root_dir: &Path, entry: &AuditEventEntry<'_>) {
    if let Err(err) = append_audit_event(data_root_dir, entry) {
        AUDIT_WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let last = AUDIT_LAST_ERROR_LOG_MS.load(Ordering::Relaxed);
        if now_ms.saturating_sub(last) > 30_000
            && AUDIT_LAST_ERROR_LOG_MS
                .compare_exchange(last, now_ms, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
        {
            tracing::warn!("audit write failed (rate-limited): {err:#}");
        }
    }
}

pub fn append_audit_event(data_root_dir: &Path, entry: &AuditEventEntry<'_>) -> Result<()> {
    let dir = data_root_dir.join("logs");
    create_dir_all(&dir)?;
    let db_path = dir.join("audit.db");
    let conn = Connection::open(db_path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS audit_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS audit_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            turn_id TEXT NULL,
            ts_ms INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            severity TEXT NOT NULL,
            schema_version INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            prev_hash TEXT NULL,
            event_hash TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_audit_session_ts ON audit_events(session_id, ts_ms);
        CREATE INDEX IF NOT EXISTS idx_audit_type_ts ON audit_events(event_type, ts_ms);",
    )?;
    conn.execute(
        "INSERT INTO audit_meta(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![AUDIT_SCHEMA_VERSION.to_string()],
    )?;

    run_retention_if_due(&conn, data_root_dir, chrono::Utc::now().timestamp_millis())?;

    let prev_hash: Option<String> = conn
        .query_row(
            "SELECT event_hash FROM audit_events ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    let ts_ms = chrono::Utc::now().timestamp_millis();
    let event_hash = compute_hash(
        entry.session_id,
        entry.turn_id,
        ts_ms,
        entry.event_type,
        entry.severity,
        &entry.payload_json,
        prev_hash.as_deref(),
    );

    conn.execute(
        "INSERT INTO audit_events(session_id, turn_id, ts_ms, event_type, severity, schema_version, payload_json, prev_hash, event_hash)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry.session_id,
            entry.turn_id,
            ts_ms,
            entry.event_type,
            entry.severity,
            AUDIT_SCHEMA_VERSION,
            entry.payload_json,
            prev_hash,
            event_hash
        ],
    )?;

    Ok(())
}

pub fn verify_audit_chain(data_root_dir: &Path) -> Result<()> {
    let db_path = data_root_dir.join("logs").join("audit.db");
    if !db_path.exists() {
        return Ok(());
    }
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT session_id, turn_id, ts_ms, event_type, severity, payload_json, prev_hash, event_hash
         FROM audit_events ORDER BY id ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut prev: Option<String> = None;
    while let Some(row) = rows.next()? {
        let session_id: String = row.get(0)?;
        let turn_id: Option<String> = row.get(1)?;
        let ts_ms: i64 = row.get(2)?;
        let event_type: String = row.get(3)?;
        let severity: String = row.get(4)?;
        let payload_json: String = row.get(5)?;
        let stored_prev: Option<String> = row.get(6)?;
        let stored_hash: String = row.get(7)?;
        if stored_prev != prev {
            anyhow::bail!("audit chain prev_hash mismatch");
        }
        let expected = compute_hash(
            &session_id,
            turn_id.as_deref(),
            ts_ms,
            &event_type,
            &severity,
            &payload_json,
            prev.as_deref(),
        );
        if expected != stored_hash {
            anyhow::bail!("audit chain event_hash mismatch");
        }
        prev = Some(stored_hash);
    }
    Ok(())
}

fn run_retention_if_due(conn: &Connection, data_root_dir: &Path, now_ms: i64) -> Result<()> {
    let key = data_root_dir.to_string_lossy().to_string();
    let map_lock = RETENTION_LAST_RUN_MS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = map_lock.lock().expect("retention map poisoned");
    if let Some(last) = map.get(&key).copied()
        && now_ms - last < 3_600_000
    {
        return Ok(());
    }
    map.insert(key, now_ms);
    drop(map);

    let cutoff = now_ms - AUDIT_RETENTION_DAYS * 24 * 60 * 60 * 1000;
    conn.execute("DELETE FROM audit_events WHERE ts_ms < ?1", params![cutoff])?;
    Ok(())
}

fn compute_hash(
    session_id: &str,
    turn_id: Option<&str>,
    ts_ms: i64,
    event_type: &str,
    severity: &str,
    payload_json: &str,
    prev_hash: Option<&str>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut hasher);
    turn_id.hash(&mut hasher);
    ts_ms.hash(&mut hasher);
    event_type.hash(&mut hasher);
    severity.hash(&mut hasher);
    payload_json.hash(&mut hasher);
    prev_hash.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::{AuditEventEntry, append_audit_event, run_retention_if_due, verify_audit_chain};
    use rusqlite::Connection;

    fn temp_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cloudagent-audit-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn writes_audit_events_and_verifies_chain() {
        let root = temp_root();
        let a = AuditEventEntry {
            session_id: "s1",
            turn_id: Some("t1"),
            event_type: "tool.started",
            severity: "info",
            payload_json: "{\"k\":1}".to_string(),
        };
        let b = AuditEventEntry {
            session_id: "s1",
            turn_id: Some("t1"),
            event_type: "tool.completed",
            severity: "info",
            payload_json: "{\"k\":2}".to_string(),
        };
        append_audit_event(&root, &a).expect("append a");
        append_audit_event(&root, &b).expect("append b");
        verify_audit_chain(&root).expect("verify chain");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn retention_deletes_old_rows() {
        let root = temp_root();
        let e = AuditEventEntry {
            session_id: "s2",
            turn_id: None,
            event_type: "turn.started",
            severity: "info",
            payload_json: "{}".to_string(),
        };
        append_audit_event(&root, &e).expect("append event");
        let db = root.join("logs").join("audit.db");
        let conn = Connection::open(&db).expect("open db");
        conn.execute(
            "UPDATE audit_events SET ts_ms = ts_ms - (40 * 24 * 60 * 60 * 1000)",
            [],
        )
        .expect("age row");
        run_retention_if_due(
            &conn,
            &root,
            chrono::Utc::now().timestamp_millis() + 4_000_000,
        )
        .expect("run retention");
        let old_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE ts_ms < (strftime('%s','now')*1000 - 30*24*60*60*1000)",
                [],
                |r| r.get(0),
            )
            .expect("old count");
        assert_eq!(old_count, 0);
        let _ = std::fs::remove_dir_all(root);
    }
}
