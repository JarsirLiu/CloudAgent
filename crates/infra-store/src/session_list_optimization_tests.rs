use super::JsonConversationStore;
use agent_core::EventMsg;
use agent_core::rollout::RolloutItem;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

static TEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_path(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock drift")
        .as_nanos();
    let counter = TEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{unique}-{counter}"))
}

fn turn_started(conversation_id: &str, turn_id: &str) -> RolloutItem {
    RolloutItem::EventMsg {
        event: EventMsg::TurnStarted {
            turn_id: turn_id.to_string(),
            conversation_id: conversation_id.to_string(),
            user_input: Vec::new(),
        },
    }
}

#[tokio::test]
async fn conversation_list_page_uses_keyset_cursor() {
    let root = unique_temp_path("cloudagent-session-page");
    let store = JsonConversationStore::new(&root);

    for id in ["session-a", "session-b", "session-c"] {
        store
            .append_rollout_items(id, &[turn_started(id, &format!("turn-{id}"))])
            .await
            .expect("append rollout");
    }

    let first = store
        .list_conversations_page(None, 2)
        .await
        .expect("first page");
    assert_eq!(first.conversations.len(), 2);
    assert!(first.has_more);
    let cursor = first.next_cursor.clone().expect("next cursor");

    let second = store
        .list_conversations_page(Some(cursor), 2)
        .await
        .expect("second page");
    assert_eq!(second.conversations.len(), 1);
    assert!(!second.has_more);

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn reconcile_missing_conversations_removes_deleted_rollout_metadata() {
    let root = unique_temp_path("cloudagent-session-reconcile");
    let store = JsonConversationStore::new(&root);
    let conversation_id = "session-missing-rollout";

    store
        .append_rollout_items(conversation_id, &[turn_started(conversation_id, "turn-1")])
        .await
        .expect("append rollout");
    let rollout = store
        .root()
        .join(format!("{conversation_id}.rollout.jsonl"));
    fs::remove_file(&rollout).await.expect("remove rollout");

    let report = store
        .reconcile_missing_conversations(100)
        .await
        .expect("reconcile");

    assert_eq!(report.checked, 1);
    assert_eq!(report.removed, vec![conversation_id.to_string()]);
    assert!(
        store
            .list_conversations()
            .await
            .expect("list after reconcile")
            .is_empty()
    );

    let _ = fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn reconcile_keeps_empty_timestamp_placeholder_without_rollout() {
    let root = unique_temp_path("cloudagent-session-placeholder");
    let store = JsonConversationStore::new(&root);
    let placeholder_id = "20260609-120000-abcd";

    store
        .create_conversation(placeholder_id)
        .await
        .expect("create placeholder");

    let report = store
        .reconcile_missing_conversations(100)
        .await
        .expect("reconcile");

    assert!(report.removed.is_empty());
    assert!(
        store
            .has_conversation(placeholder_id)
            .await
            .expect("placeholder still indexed")
    );
    assert!(
        store
            .list_conversations()
            .await
            .expect("list hides placeholder")
            .is_empty()
    );

    let _ = fs::remove_dir_all(root).await;
}
