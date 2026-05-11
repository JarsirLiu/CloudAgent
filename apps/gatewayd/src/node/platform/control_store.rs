use super::state::{
    PlatformControlState, default_state, normalize_state, platform_control_db_path,
    platform_control_legacy_json_path, supported_platforms,
};
use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::Path;

pub(crate) async fn load_control_state(
    data_root_dir: Option<&std::ffi::OsStr>,
) -> Result<PlatformControlState> {
    let db_path = platform_control_db_path(data_root_dir);
    let legacy_path = platform_control_legacy_json_path(data_root_dir);

    tokio::task::spawn_blocking(move || load_control_state_blocking(&db_path, &legacy_path)).await?
}

pub(crate) async fn persist_control_state(
    data_root_dir: Option<&std::ffi::OsStr>,
    state: &PlatformControlState,
) -> Result<()> {
    let db_path = platform_control_db_path(data_root_dir);
    let legacy_path = platform_control_legacy_json_path(data_root_dir);
    let state = state.clone();

    tokio::task::spawn_blocking(move || {
        persist_control_state_blocking(&db_path, &legacy_path, &state)
    })
    .await?
}

fn load_control_state_blocking(db_path: &Path, legacy_path: &Path) -> Result<PlatformControlState> {
    if !db_path.exists() && legacy_path.exists() {
        let text = std::fs::read_to_string(legacy_path)?;
        let legacy = normalize_state(serde_json::from_str(&text)?);
        persist_control_state_blocking(db_path, legacy_path, &legacy)?;
        return Ok(legacy);
    }

    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    initialize_schema(&conn)?;

    let mut state = default_state();
    let mut stmt = conn.prepare(
        "SELECT platform, enabled, managed_by, updated_at_ms
         FROM platform_control",
    )?;
    let rows = stmt.query_map([], |row| {
        let platform: String = row.get(0)?;
        let enabled: bool = row.get(1)?;
        let managed_by: String = row.get(2)?;
        let updated_at_ms: u64 = row.get(3)?;
        Ok((platform, enabled, managed_by, updated_at_ms))
    })?;

    for row in rows {
        let (platform, enabled, managed_by, updated_at_ms) = row?;
        if let Some(entry) = state.platforms.get_mut(&platform) {
            entry.enabled = enabled;
            entry.managed_by = managed_by;
            entry.updated_at_ms = updated_at_ms;
        }
    }

    Ok(state)
}

fn persist_control_state_blocking(
    db_path: &Path,
    legacy_path: &Path,
    state: &PlatformControlState,
) -> Result<()> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut conn = Connection::open(db_path)?;
    initialize_schema(&conn)?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM platform_control", [])?;
    for platform in supported_platforms() {
        if let Some(entry) = state.platforms.get(*platform) {
            tx.execute(
                "INSERT INTO platform_control (platform, enabled, managed_by, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    entry.platform,
                    entry.enabled,
                    entry.managed_by,
                    entry.updated_at_ms
                ],
            )?;
        }
    }
    tx.commit()?;

    if legacy_path.exists() {
        let _ = std::fs::remove_file(legacy_path);
    }

    Ok(())
}

fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS platform_control (
            platform TEXT PRIMARY KEY NOT NULL,
            enabled INTEGER NOT NULL,
            managed_by TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL
        );",
    )?;
    Ok(())
}
