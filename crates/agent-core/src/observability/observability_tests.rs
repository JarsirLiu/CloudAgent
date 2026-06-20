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
