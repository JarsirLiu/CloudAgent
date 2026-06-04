use super::types::{NodeEvent, WorkerHandle, record_worker_fault, should_evict_worker};
use super::{IDLE_WORKER_TTL, WorkerManager};
use crate::node::test_support::test_worker_program;
use agent_core::conversation::ConversationSummary;
use agent_protocol::{
    CommandExecutionContext, ConversationListResponse, JsonRpcRequest, NodeWorkerHealth, RequestId,
};
use anyhow::Result;
use infra_store::JsonConversationStore;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;

#[test]
fn builds_shared_worker_stdio_arguments() {
    assert_eq!(
        super::transport::worker_stdio_args(None),
        vec![OsString::from("app-server-stdio"),]
    );
}

#[test]
fn worker_stdio_arguments_include_data_root_when_present() {
    assert_eq!(
        super::transport::worker_stdio_args(Some(OsString::from("D:\\cloudagent-data"))),
        vec![
            OsString::from("app-server-stdio"),
            OsString::from("--data-dir"),
            OsString::from("D:\\cloudagent-data"),
        ]
    );
}

#[test]
fn normalizes_worker_disconnect_messages() {
    assert_eq!(
        super::transport::normalize_worker_disconnect_message("stdio app server closed"),
        "ERR_TRANSPORT_CLOSED: worker app server closed unexpectedly"
    );
    assert_eq!(
        super::transport::normalize_worker_disconnect_message("local node closed"),
        "ERR_TRANSPORT_CLOSED: local node closed"
    );
}

#[tokio::test]
async fn prune_finished_worker_removes_completed_handle() -> Result<()> {
    let manager = WorkerManager::new(test_worker_program(), None);
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    let worker = tokio::spawn(async { Result::<()>::Ok(()) });
    {
        let mut state = manager.state.lock().await;
        let (events_tx, _) = broadcast::channel(8);
        state.workers.insert(
            "session-1".to_string(),
            WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now(),
            },
        );
    }

    tokio::task::yield_now().await;
    manager.prune_workers().await?;
    assert!(manager.state.lock().await.workers.is_empty());
    Ok(())
}

#[tokio::test]
async fn subscribe_receives_broadcast_events_from_existing_shared_worker() -> Result<()> {
    let manager = WorkerManager::new(test_worker_program(), None);
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    let worker = tokio::spawn(async {
        std::future::pending::<()>().await;
        #[allow(unreachable_code)]
        Result::<()>::Ok(())
    });
    let (events_tx, _) = broadcast::channel(8);
    {
        let mut state = manager.state.lock().await;
        state.workers.insert(
            "session-1".to_string(),
            WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx: events_tx.clone(),
                worker,
                last_active_at: Instant::now(),
            },
        );
    }

    let mut receiver = manager.subscribe("session-1", "conversation-1").await?;
    let _ = events_tx.send(NodeEvent::Diagnostic {
        conversation_id: "default".to_string(),
        message: "hello".to_string(),
        is_error: false,
    });

    match receiver.recv().await? {
        NodeEvent::Diagnostic {
            conversation_id,
            message,
            is_error,
        } => {
            assert_eq!(conversation_id, "default");
            assert_eq!(message, "hello");
            assert!(!is_error);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn prune_worker_evicts_idle_handle() -> Result<()> {
    let manager = WorkerManager::new(test_worker_program(), None);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let worker = tokio::spawn(async move {
        while rx.recv().await.is_some() {}
        Result::<()>::Ok(())
    });
    {
        let mut state = manager.state.lock().await;
        let (events_tx, _) = broadcast::channel(8);
        state.workers.insert(
            "session-1".to_string(),
            WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now() - IDLE_WORKER_TTL,
            },
        );
    }

    manager.prune_workers_at(Instant::now()).await?;
    assert!(manager.state.lock().await.workers.is_empty());
    Ok(())
}

#[tokio::test]
async fn prune_worker_keeps_recent_handle() -> Result<()> {
    let manager = WorkerManager::new(test_worker_program(), None);
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    let worker = tokio::spawn(async {
        std::future::pending::<()>().await;
        #[allow(unreachable_code)]
        Result::<()>::Ok(())
    });
    {
        let mut state = manager.state.lock().await;
        let (events_tx, _) = broadcast::channel(8);
        state.workers.insert(
            "session-1".to_string(),
            WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now(),
            },
        );
        let handle = state.workers.get("session-1").expect("worker handle");
        assert!(!should_evict_worker(
            handle,
            Instant::now(),
            IDLE_WORKER_TTL
        ));
    }

    manager.prune_workers_at(Instant::now()).await?;
    assert!(manager.state.lock().await.workers.contains_key("session-1"));
    Ok(())
}

#[tokio::test]
async fn is_worker_running_prunes_idle_handle() -> Result<()> {
    let manager = WorkerManager::new(test_worker_program(), None);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let worker = tokio::spawn(async move {
        while rx.recv().await.is_some() {}
        Result::<()>::Ok(())
    });
    {
        let mut state = manager.state.lock().await;
        let (events_tx, _) = broadcast::channel(8);
        state.workers.insert(
            "session-1".to_string(),
            WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now() - IDLE_WORKER_TTL,
            },
        );
    }

    assert!(!manager.is_worker_running().await);
    assert!(manager.state.lock().await.workers.is_empty());
    Ok(())
}

#[tokio::test]
async fn status_snapshot_reports_running_and_faulted_scopes() -> Result<()> {
    let manager = WorkerManager::new(test_worker_program(), None);
    let (tx, rx) = mpsc::unbounded_channel();
    drop(rx);
    let worker = tokio::spawn(async {
        std::future::pending::<()>().await;
        #[allow(unreachable_code)]
        Result::<()>::Ok(())
    });
    {
        let mut state = manager.state.lock().await;
        let (events_tx, _) = broadcast::channel(8);
        state.workers.insert(
            "local:cli".to_string(),
            WorkerHandle {
                command_tx: tx,
                request_tx: mpsc::unbounded_channel().0,
                events_tx,
                worker,
                last_active_at: Instant::now(),
            },
        );
    }
    record_worker_fault(&manager.state, "im:feishu", "transport failed".to_string()).await;

    let snapshot = manager.status_snapshot().await;

    assert_eq!(snapshot.len(), 2);
    assert!(snapshot.iter().any(|status| {
        status.worker_scope_key == "local:cli" && matches!(status.health, NodeWorkerHealth::Running)
    }));
    assert!(snapshot.iter().any(|status| {
        status.worker_scope_key == "im:feishu"
            && matches!(status.health, NodeWorkerHealth::Faulted)
            && status.detail.as_deref() == Some("transport failed")
    }));
    Ok(())
}

#[tokio::test]
async fn shared_worker_process_switches_runtime_by_request_context() -> Result<()> {
    let manager = WorkerManager::new(test_worker_program(), None);
    let workspace_a = unique_workspace("worker-runtime-a");
    let workspace_b = unique_workspace("worker-runtime-b");
    seed_workspace(&workspace_a, "conversation-a").await?;
    seed_workspace(&workspace_b, "conversation-b").await?;

    let response_a: ConversationListResponse = serde_json::from_value(
        manager
            .request_json(
                "local:cli-shared",
                "default",
                list_conversations_request(1),
                Some(context_for_workspace(&workspace_a)),
            )
            .await?,
    )?;
    let response_b: ConversationListResponse = serde_json::from_value(
        manager
            .request_json(
                "local:cli-shared",
                "default",
                list_conversations_request(2),
                Some(context_for_workspace(&workspace_b)),
            )
            .await?,
    )?;

    assert_eq!(
        conversation_ids(&response_a),
        BTreeSet::from(["conversation-a".to_string()])
    );
    assert_eq!(
        conversation_ids(&response_b),
        BTreeSet::from(["conversation-b".to_string()])
    );

    let statuses = manager.status_snapshot().await;
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].worker_scope_key, "local:cli-shared");
    assert!(matches!(statuses[0].health, NodeWorkerHealth::Running));

    manager.shutdown().await?;
    Ok(())
}

fn list_conversations_request(id: i64) -> JsonRpcRequest {
    JsonRpcRequest {
        id: RequestId::Integer(id),
        method: "conversation/list".to_string(),
        params: Some(serde_json::Value::Null),
    }
}

fn context_for_workspace(workspace_root: &Path) -> CommandExecutionContext {
    CommandExecutionContext {
        session_id: Some(format!(
            "session-{}",
            workspace_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("workspace")
        )),
        workspace_id: None,
        workspace_root: Some(workspace_root.to_string_lossy().into_owned()),
        cwd: Some(workspace_root.to_string_lossy().into_owned()),
        permission_mode: Some("WorkspaceWrite".to_string()),
        data_root_dir: Some(workspace_root.join("data").to_string_lossy().into_owned()),
    }
}

async fn seed_workspace(workspace_root: &Path, conversation_id: &str) -> Result<()> {
    tokio::fs::create_dir_all(workspace_root.join("configs")).await?;
    tokio::fs::create_dir_all(workspace_root.join("data").join("conversations")).await?;
    tokio::fs::create_dir_all(workspace_root.join("data").join("state").join("memory")).await?;
    let store = JsonConversationStore::new(workspace_root.join("data").join("conversations"));
    store.create_conversation(conversation_id).await?;
    Ok(())
}

fn conversation_ids(response: &ConversationListResponse) -> BTreeSet<String> {
    response
        .conversations
        .iter()
        .map(|summary: &ConversationSummary| summary.conversation_id.clone())
        .collect()
}

fn unique_workspace(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("cloudagent-worker-test-{label}-{unique}"))
}
