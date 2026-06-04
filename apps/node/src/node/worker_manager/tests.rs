use super::types::{NodeEvent, WorkerHandle, record_worker_fault, should_evict_worker};
use super::{IDLE_WORKER_TTL, WorkerManager};
use crate::node::test_support::test_worker_program;
use agent_protocol::NodeWorkerHealth;
use anyhow::Result;
use std::ffi::OsString;
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
