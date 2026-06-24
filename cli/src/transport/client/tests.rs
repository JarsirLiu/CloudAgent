use super::{
    connect_local_node_once, create_local_node_client, existing_node_looks_unhealthy,
    wait_for_service,
};
use agent_protocol::{
    JsonRpcMessage, JsonRpcResponse, NodeStatusResponse, SessionBootstrapContext,
    TransportInitializeParams, TransportInitializeResult, TransportServerInfo,
};
use anyhow::{Result, anyhow};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use std::{ffi::OsString, path::PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn waits_for_service_until_it_becomes_reachable() {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
    let addr = probe.local_addr().expect("probe local addr");
    drop(probe);

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let listener = TcpListener::bind(addr)
            .await
            .expect("bind delayed listener");
        let _ = listener.accept().await.expect("accept one client");
    });

    let attempts = Arc::new(AtomicUsize::new(0));
    let result = wait_for_service(
        {
            let attempts = attempts.clone();
            move || {
                let attempts = attempts.clone();
                async move {
                    attempts.fetch_add(1, Ordering::Relaxed);
                    TcpStream::connect(addr)
                        .await
                        .map(|_| ())
                        .map_err(|err| anyhow!(err))
                }
            }
        },
        None,
        Duration::from_secs(2),
        Duration::from_millis(50),
    )
    .await;

    assert!(
        result.is_ok(),
        "service should become reachable: {result:?}"
    );
    assert!(
        attempts.load(Ordering::Relaxed) >= 1,
        "should attempt to connect at least once"
    );
}

#[tokio::test]
async fn times_out_when_service_never_starts() {
    let result: Result<()> = wait_for_service(
        || async { Err(anyhow!("connection refused")) },
        None,
        Duration::from_millis(150),
        Duration::from_millis(25),
    )
    .await;

    let error = result.expect_err("service should time out");
    assert!(
        error
            .to_string()
            .contains("service did not become reachable within"),
        "unexpected timeout error: {error}"
    );
}

#[test]
fn detects_unhealthy_existing_node_errors() {
    assert!(existing_node_looks_unhealthy(&anyhow!(
        "local node closed during initialize"
    )));
    assert!(existing_node_looks_unhealthy(&anyhow!(
        "local node initialize failed: initialize denied"
    )));
    assert!(existing_node_looks_unhealthy(&anyhow!(
        "timed out initializing local node at 127.0.0.1:47070"
    )));
    assert!(!existing_node_looks_unhealthy(&anyhow!(
        "failed to connect to local node at 127.0.0.1:47070: connection refused"
    )));
}

#[tokio::test]
async fn local_node_connections_send_distinct_session_contexts_per_workspace() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake node listener");
    let address = listener.local_addr().expect("listener addr").to_string();

    let expected_contexts = vec![
        (
            "D:/repo-alpha".to_string(),
            "D:/repo-alpha".to_string(),
            "D:/repo-alpha/data".to_string(),
        ),
        (
            "D:/repo-beta".to_string(),
            "D:/repo-beta".to_string(),
            "D:/repo-beta/data".to_string(),
        ),
    ];
    let server = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.expect("accept connection");
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            let mut initialize_line = String::new();
            reader
                .read_line(&mut initialize_line)
                .await
                .expect("read initialize");
            let JsonRpcMessage::Request(request) =
                serde_json::from_str(initialize_line.trim_end()).expect("parse initialize")
            else {
                panic!("expected initialize request");
            };
            assert_eq!(request.method, "initialize");
            let params: TransportInitializeParams =
                serde_json::from_value(request.params.expect("initialize params"))
                    .expect("decode initialize params");
            let context = params.session_context.expect("session context");
            seen.push((
                context.workspace_root.expect("workspace root"),
                context.cwd.expect("cwd"),
                context.data_root_dir.expect("data root dir"),
            ));

            let initialize_response = JsonRpcMessage::Response(JsonRpcResponse {
                id: request.id,
                result: serde_json::to_value(TransportInitializeResult {
                    server_info: TransportServerInfo {
                        name: "node".to_string(),
                        version: "0.0.0-test".to_string(),
                    },
                    protocol_version: "1".to_string(),
                    transport: "remote".to_string(),
                })
                .expect("serialize initialize result"),
            });
            let payload = serde_json::to_string(&initialize_response).expect("serialize response");
            writer
                .write_all(payload.as_bytes())
                .await
                .expect("write initialize response");
            writer.write_all(b"\n").await.expect("write newline");

            let mut initialized_line = String::new();
            reader
                .read_line(&mut initialized_line)
                .await
                .expect("read initialized");
            let JsonRpcMessage::Notification(notification) =
                serde_json::from_str(initialized_line.trim_end())
                    .expect("parse initialized notification")
            else {
                panic!("expected initialized notification");
            };
            assert_eq!(notification.method, "initialized");
        }
        seen
    });

    let client_a = connect_local_node_once(
        &address,
        Some(SessionBootstrapContext {
            session_id: Some("session-alpha".to_string()),
            source_domain: Some("local:cli".to_string()),
            workspace_root: Some("D:/repo-alpha".to_string()),
            cwd: Some("D:/repo-alpha".to_string()),
            permission_mode: Some("WorkspaceWrite".to_string()),
            data_root_dir: Some("D:/repo-alpha/data".to_string()),
        }),
    )
    .await
    .expect("connect alpha client");
    let client_b = connect_local_node_once(
        &address,
        Some(SessionBootstrapContext {
            session_id: Some("session-beta".to_string()),
            source_domain: Some("local:cli".to_string()),
            workspace_root: Some("D:/repo-beta".to_string()),
            cwd: Some("D:/repo-beta".to_string()),
            permission_mode: Some("WorkspaceWrite".to_string()),
            data_root_dir: Some("D:/repo-beta/data".to_string()),
        }),
    )
    .await
    .expect("connect beta client");

    client_a.shutdown().await.expect("shutdown alpha client");
    client_b.shutdown().await.expect("shutdown beta client");

    let seen = server.await.expect("fake node server task");
    assert_eq!(seen, expected_contexts);
}

#[tokio::test]
async fn create_local_node_client_preserves_workspace_context_and_data_root() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake node listener");
    let address = listener.local_addr().expect("listener addr").to_string();
    let expected_data_root = PathBuf::from("D:/shared-node-data");
    let expected_contexts = vec![
        (
            "session-alpha".to_string(),
            "D:/repo-alpha".to_string(),
            "D:/repo-alpha".to_string(),
            "D:/shared-node-data".to_string(),
        ),
        (
            "session-beta".to_string(),
            "D:/repo-beta".to_string(),
            "D:/repo-beta".to_string(),
            "D:/shared-node-data".to_string(),
        ),
    ];
    let server = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.expect("accept connection");
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            let mut initialize_line = String::new();
            reader
                .read_line(&mut initialize_line)
                .await
                .expect("read initialize");
            let JsonRpcMessage::Request(request) =
                serde_json::from_str(initialize_line.trim_end()).expect("parse initialize")
            else {
                panic!("expected initialize request");
            };
            assert_eq!(request.method, "initialize");
            let params: TransportInitializeParams =
                serde_json::from_value(request.params.expect("initialize params"))
                    .expect("decode initialize params");
            let context = params.session_context.expect("session context");
            seen.push((
                context.session_id.expect("session id"),
                context.workspace_root.expect("workspace root"),
                context.cwd.expect("cwd"),
                context.data_root_dir.expect("data root dir"),
            ));

            let initialize_response = JsonRpcMessage::Response(JsonRpcResponse {
                id: request.id,
                result: serde_json::to_value(TransportInitializeResult {
                    server_info: TransportServerInfo {
                        name: "node".to_string(),
                        version: "0.0.0-test".to_string(),
                    },
                    protocol_version: "1".to_string(),
                    transport: "remote".to_string(),
                })
                .expect("serialize initialize result"),
            });
            let payload = serde_json::to_string(&initialize_response).expect("serialize response");
            writer
                .write_all(payload.as_bytes())
                .await
                .expect("write initialize response");
            writer.write_all(b"\n").await.expect("write newline");

            let mut initialized_line = String::new();
            reader
                .read_line(&mut initialized_line)
                .await
                .expect("read initialized");
            let JsonRpcMessage::Notification(notification) =
                serde_json::from_str(initialized_line.trim_end())
                    .expect("parse initialized notification")
            else {
                panic!("expected initialized notification");
            };
            assert_eq!(notification.method, "initialized");

            let mut status_line = String::new();
            reader
                .read_line(&mut status_line)
                .await
                .expect("read node status request");
            let JsonRpcMessage::Request(request) =
                serde_json::from_str(status_line.trim_end()).expect("parse node status request")
            else {
                panic!("expected node/status request");
            };
            assert_eq!(request.method, "node/status");
            let status_response = JsonRpcMessage::Response(JsonRpcResponse {
                id: request.id,
                result: serde_json::to_value(NodeStatusResponse {
                    listen_address: "127.0.0.1:47070".to_string(),
                    worker_running: false,
                    platform_runtime_count: 0,
                    managed_platform_count: 0,
                    data_root_dir: "D:/shared-node-data".to_string(),
                    conversation_store_dir: "D:/shared-node-data/conversations".to_string(),
                    workers: Vec::new(),
                })
                .expect("serialize node status response"),
            });
            let payload =
                serde_json::to_string(&status_response).expect("serialize status response");
            writer
                .write_all(payload.as_bytes())
                .await
                .expect("write node status response");
            writer.write_all(b"\n").await.expect("write newline");
        }
        seen
    });

    let client_a = create_local_node_client(
        &address,
        &OsString::from("node.exe"),
        &[],
        &expected_data_root,
        Some(SessionBootstrapContext {
            session_id: Some("session-alpha".to_string()),
            source_domain: Some("local:cli".to_string()),
            workspace_root: Some("D:/repo-alpha".to_string()),
            cwd: Some("D:/repo-alpha".to_string()),
            permission_mode: Some("WorkspaceWrite".to_string()),
            data_root_dir: Some("D:/shared-node-data".to_string()),
        }),
    )
    .await
    .expect("connect alpha client");
    let client_b = create_local_node_client(
        &address,
        &OsString::from("node.exe"),
        &[],
        &expected_data_root,
        Some(SessionBootstrapContext {
            session_id: Some("session-beta".to_string()),
            source_domain: Some("local:cli".to_string()),
            workspace_root: Some("D:/repo-beta".to_string()),
            cwd: Some("D:/repo-beta".to_string()),
            permission_mode: Some("WorkspaceWrite".to_string()),
            data_root_dir: Some("D:/shared-node-data".to_string()),
        }),
    )
    .await
    .expect("connect beta client");

    client_a.shutdown().await.expect("shutdown alpha client");
    client_b.shutdown().await.expect("shutdown beta client");

    let seen = server.await.expect("fake node server task");
    assert_eq!(seen, expected_contexts);
}
