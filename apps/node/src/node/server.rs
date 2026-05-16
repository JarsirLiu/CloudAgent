use crate::node::command_router::handle_command_message;
use crate::node::data_root::resolve_data_root_dir;
use crate::node::device_settings::conversation_store_dir;
use crate::node::message_sync::write_node_event;
use crate::node::platform::PlatformManager;
use crate::node::runtime::NodeRuntime;
use crate::node::session_state::NodeSessionState;
use crate::node::source::NodeSource;
use crate::node::worker_manager::NodeEvent;
use agent_core::SkillRuntime;
use agent_protocol::{
    JsonRpcError, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcNotification, JsonRpcResponse,
    TransportInitializeParams, TransportInitializeResult, TransportServerInfo,
};
use anyhow::{Context, Result, anyhow};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) async fn run_resident_node(args: &[OsString]) -> Result<()> {
    let listen_address = arg_value(args, "--listen")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(default_listen_address);
    let worker_program = resolve_worker_program(args)?;
    let data_root_dir =
        arg_value(args, "--data-dir").or_else(|| std::env::var_os("CLOUDAGENT_DATA_ROOT_DIR"));
    let resolved_data_root_dir = resolve_data_root_dir(data_root_dir.as_deref());
    let workspace_root =
        std::env::current_dir().context("failed to resolve node workspace root")?;
    let skill_runtime = load_node_skill_runtime(&workspace_root);

    let listener = TcpListener::bind(&listen_address)
        .await
        .with_context(|| format!("failed to bind node remote host on {listen_address}"))?;
    tracing::info!("node remote app-server host listening on {listen_address}");
    tracing::info!(
        data_root_dir = %resolved_data_root_dir.display(),
        conversation_store_dir = %conversation_store_dir(Some(resolved_data_root_dir.as_os_str())).display(),
        "node data root resolved"
    );
    if should_emit_dev_launch_logs() {
        tracing::info!(
            worker_program = %PathBuf::from(&worker_program).display(),
            "node development worker resolved"
        );
    }
    let platforms = PlatformManager::load(Some(resolved_data_root_dir.as_os_str())).await?;
    let conversation_store = infra_store::JsonConversationStore::new(conversation_store_dir(Some(
        resolved_data_root_dir.as_os_str(),
    )));
    let runtime = NodeRuntime::new(
        crate::node::worker_manager::WorkerManager::new(
            worker_program,
            Some(resolved_data_root_dir.clone().into_os_string()),
        ),
        conversation_store,
        platforms,
        listen_address.clone(),
        workspace_root,
        skill_runtime,
        resolved_data_root_dir,
    );
    let platform_runtime = runtime.clone();
    let platform_listen_address = listen_address.clone();
    tokio::spawn(async move {
        if let Err(error) = platform_runtime
            .platforms()
            .sync_desired_state(&platform_listen_address)
            .await
        {
            tracing::warn!("failed to sync desired platform state: {error}");
        }
    });

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (stream, peer_addr) = accept?;
                tracing::debug!("accepted remote app-server client from {peer_addr}");
                let runtime = runtime.clone();
                let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let (reader, writer) = stream.into_split();
                    if let Err(error) = run_connection(
                        BufReader::new(reader),
                        writer,
                        runtime,
                        format!("remote:{peer_addr}#{session_id}"),
                    )
                    .await
                    {
                        tracing::warn!("node remote host connection failed: {error}");
                    }
                });
            }
            _ = runtime.wait_for_shutdown() => {
                tracing::info!("node received node shutdown request");
                break;
            }
        }
    }

    runtime.shutdown().await?;
    Ok(())
}

fn load_node_skill_runtime(workspace_root: &Path) -> SkillRuntime {
    match config::AgentConfig::load_user_only(workspace_root.to_path_buf()) {
        Ok(config) => SkillRuntime::new(
            config.runtime.skills_enabled,
            config.runtime.skill_roots.clone(),
        ),
        Err(error) => {
            tracing::warn!(
                "failed to load node skill config, falling back to default roots: {error:#}"
            );
            SkillRuntime::new(true, Vec::new())
        }
    }
}

async fn run_connection<R, W>(
    reader: R,
    mut writer: W,
    runtime: NodeRuntime,
    worker_scope_key: String,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut input_lines = reader.lines();
    let mut session = NodeSessionState::new("default", worker_scope_key);

    loop {
        if let Some(subscription) = session.active_subscription_mut().as_mut() {
            tokio::select! {
                maybe_line = input_lines.next_line() => {
                    match maybe_line.context("failed to read remote app-server command line")? {
                        Some(line) => {
                            if !handle_transport_line(&line, &runtime, &mut session, &mut writer)
                                .await?
                            {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                maybe_event = subscription.recv() => {
                    match maybe_event {
                        Ok(event) => {
                            if session.should_forward_event(&event) {
                                write_node_event(&mut writer, event, &runtime).await?;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            *session.active_subscription_mut() = None;
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            write_node_event(
                                &mut writer,
                                NodeEvent::Diagnostic {
                                    conversation_id: session.active_conversation_id().to_string(),
                                    message: format!(
                                        "remote app-server subscriber lagged; skipped {skipped} events"
                                    ),
                                    is_error: false,
                                },
                                &runtime,
                            )
                            .await?;
                        }
                    }
                }
            }
        } else {
            match input_lines
                .next_line()
                .await
                .context("failed to read remote app-server command line")?
            {
                Some(line) => {
                    if !handle_transport_line(&line, &runtime, &mut session, &mut writer).await? {
                        break;
                    }
                }
                None => break,
            }
        }
    }

    Ok(())
}

async fn handle_transport_line<W>(
    line: &str,
    runtime: &NodeRuntime,
    session: &mut NodeSessionState,
    writer: &mut W,
) -> Result<bool>
where
    W: AsyncWrite + Unpin,
{
    let rpc: JsonRpcMessage = serde_json::from_str(line)
        .context("failed to parse remote app-server transport jsonrpc")?;

    if !session.is_ready() {
        return handle_handshake_message(rpc, session, writer).await;
    }

    match rpc {
        JsonRpcMessage::Request(request) => {
            handle_command_message(JsonRpcMessage::Request(request), runtime, session, writer).await
        }
        JsonRpcMessage::Notification(notification) => {
            handle_command_message(
                JsonRpcMessage::Notification(notification),
                runtime,
                session,
                writer,
            )
            .await
        }
        JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {
            anyhow::bail!("unexpected remote app-server transport response from client")
        }
    }
}

async fn handle_handshake_message<W>(
    rpc: JsonRpcMessage,
    session: &mut NodeSessionState,
    writer: &mut W,
) -> Result<bool>
where
    W: AsyncWrite + Unpin,
{
    match rpc {
        JsonRpcMessage::Request(request) => {
            if request.method != "initialize" {
                write_jsonrpc_message(
                    writer,
                    JsonRpcMessage::Error(JsonRpcError {
                        id: request.id,
                        error: JsonRpcErrorPayload {
                            code: -32002,
                            message: "Not initialized".to_string(),
                            data: None,
                        },
                    }),
                )
                .await?;
                return Ok(true);
            }

            if !session.expects_initialize() {
                write_jsonrpc_message(
                    writer,
                    JsonRpcMessage::Error(JsonRpcError {
                        id: request.id,
                        error: JsonRpcErrorPayload {
                            code: -32001,
                            message: "Already initialized".to_string(),
                            data: None,
                        },
                    }),
                )
                .await?;
                return Ok(true);
            }

            let params = request.params.unwrap_or(serde_json::Value::Null);
            let initialize: TransportInitializeParams = serde_json::from_value(params)
                .context("failed to decode remote app-server initialize params")?;
            session.set_source(NodeSource::from_client_info(&initialize.client_info));
            session.mark_initialize_accepted();
            write_jsonrpc_message(
                writer,
                JsonRpcMessage::Response(JsonRpcResponse {
                    id: request.id,
                    result: serde_json::to_value(TransportInitializeResult {
                        server_info: TransportServerInfo {
                            name: "node".to_string(),
                            version: env!("CARGO_PKG_VERSION").to_string(),
                        },
                        protocol_version: "1".to_string(),
                        transport: "remote".to_string(),
                    })?,
                }),
            )
            .await?;
            Ok(true)
        }
        JsonRpcMessage::Notification(JsonRpcNotification { method, .. }) => {
            if method == "initialized" && session.expects_initialized_notification() {
                session.mark_ready();
                return Ok(true);
            }
            anyhow::bail!("remote app-server transport is not initialized")
        }
        JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {
            anyhow::bail!("unexpected remote app-server transport response from client")
        }
    }
}

async fn write_jsonrpc_message<W>(writer: &mut W, message: JsonRpcMessage) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_string(&message)?;
    writer.write_all(payload.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

fn default_listen_address() -> String {
    format!("127.0.0.1:{}", workspace_scoped_node_port())
}

fn default_worker_bin() -> OsString {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(exe_name("agentd"))))
        .map(|path| path.into_os_string())
        .unwrap_or_else(|| OsString::from(exe_name("agentd")))
}

fn resolve_worker_program(args: &[OsString]) -> Result<OsString> {
    if let Some(worker_bin) =
        arg_value(args, "--worker-bin").or_else(|| std::env::var_os("CLOUDAGENT_WORKER_BIN"))
    {
        if should_emit_dev_launch_logs() {
            tracing::info!(
                worker_program = %PathBuf::from(&worker_bin).display(),
                "node development worker selected from explicit override"
            );
        }
        return Ok(worker_bin);
    }

    if should_build_worker_via_cargo() {
        return build_workspace_worker().map(|path| path.into_os_string());
    }

    Ok(default_worker_bin())
}

fn should_build_worker_via_cargo() -> bool {
    !node_release_mode_enabled()
        && cfg!(debug_assertions)
        && std::env::current_dir().is_ok_and(|dir| dir.join("Cargo.toml").exists())
}

fn node_release_mode_enabled() -> bool {
    std::env::var("CLOUDAGENT_RELEASE_MODE").ok().as_deref() == Some("1") || !cfg!(debug_assertions)
}

fn build_workspace_worker() -> Result<PathBuf> {
    let target_dir = std::env::current_dir()
        .map(|dir| dir.join("target").join(".cloudagent-local-node"))
        .unwrap_or_else(|_| PathBuf::from("target").join(".cloudagent-local-node"));
    if should_emit_dev_launch_logs() {
        tracing::info!(
            target_dir = %target_dir.display(),
            "node development worker build requested"
        );
    }
    let status = std::process::Command::new("cargo")
        .args([
            OsString::from("build"),
            OsString::from("-p"),
            OsString::from("agentd"),
            OsString::from("--target-dir"),
            target_dir.clone().into_os_string(),
        ])
        .status()
        .context("failed to build worker toolchain via cargo")?;
    if !status.success() {
        return Err(anyhow!(
            "failed to build worker toolchain via cargo (status: {status})"
        ));
    }
    let worker_path = target_dir.join(debug_exe_name("agentd"));
    if should_emit_dev_launch_logs() {
        tracing::info!(
            worker_program = %worker_path.display(),
            "node development worker build completed"
        );
    }
    Ok(worker_path)
}

fn should_emit_dev_launch_logs() -> bool {
    !node_release_mode_enabled()
}

fn debug_exe_name(base: &str) -> PathBuf {
    Path::new("debug").join(exe_name(base))
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn arg_value(args: &[OsString], name: &str) -> Option<OsString> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn workspace_scoped_node_port() -> u16 {
    let cwd = std::env::current_dir()
        .ok()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cloudagent".to_string());
    let hash = cwd.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(16777619).wrapping_add(u32::from(byte))
    });
    47070 + (hash % 1000) as u16
}

#[cfg(test)]
mod tests {
    use super::{arg_value, handle_transport_line};
    use crate::node::platform::PlatformManager;
    use crate::node::runtime::NodeRuntime;
    use crate::node::session_state::NodeSessionState;
    use crate::node::test_support::{test_worker_program, unique_temp_path};
    use crate::node::worker_manager::WorkerManager;
    use agent_core::SkillRuntime;
    use agent_protocol::{
        JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RequestId,
        TransportInitializeParams,
    };
    use std::ffi::OsString;
    use tokio::io::{AsyncBufReadExt, BufReader, duplex};

    fn test_worker_manager() -> WorkerManager {
        let root = unique_temp_path("cloudagent-node-tests");
        WorkerManager::new(test_worker_program(), Some(root.into_os_string()))
    }

    async fn test_runtime() -> NodeRuntime {
        let root = unique_temp_path("cloudagent-node-platform-tests");
        let platforms = PlatformManager::load(Some(root.as_os_str()))
            .await
            .expect("platform manager");
        NodeRuntime::new(
            test_worker_manager(),
            infra_store::JsonConversationStore::new(root.join("conversations")),
            platforms,
            "127.0.0.1:47070",
            root.clone(),
            SkillRuntime::new(true, Vec::new()),
            root,
        )
    }

    #[test]
    fn parses_serve_flag_values() {
        let args = vec![
            OsString::from("--listen"),
            OsString::from("127.0.0.1:47070"),
            OsString::from("--worker-bin"),
            OsString::from("agentd.exe"),
            OsString::from("--data-dir"),
            OsString::from("D:\\cloudagent-data"),
        ];
        assert_eq!(
            arg_value(&args, "--listen"),
            Some(OsString::from("127.0.0.1:47070"))
        );
        assert_eq!(
            arg_value(&args, "--worker-bin"),
            Some(OsString::from("agentd.exe"))
        );
        assert_eq!(
            arg_value(&args, "--data-dir"),
            Some(OsString::from("D:\\cloudagent-data"))
        );
    }

    #[tokio::test]
    async fn rejects_business_requests_before_initialize() {
        let runtime = test_runtime().await;
        let mut session = NodeSessionState::new("default", "session-test-1");
        let (mut writer, reader) = duplex(4096);
        let request = serde_json::to_string(&JsonRpcMessage::Request(JsonRpcRequest {
            id: RequestId::Integer(1),
            method: "conversation/list".to_string(),
            params: None,
        }))
        .expect("serialize request");

        handle_transport_line(&request, &runtime, &mut session, &mut writer)
            .await
            .expect("request should return a protocol error response");

        let mut line = String::new();
        let mut reader = BufReader::new(reader);
        reader.read_line(&mut line).await.expect("read response");
        let JsonRpcMessage::Error(JsonRpcError { error, .. }) =
            serde_json::from_str(line.trim_end()).expect("parse response")
        else {
            panic!("expected jsonrpc error");
        };
        assert_eq!(error.message, "Not initialized");
        assert!(!session.is_ready());
    }

    #[tokio::test]
    async fn initialize_then_initialized_marks_session_ready() {
        let runtime = test_runtime().await;
        let mut session = NodeSessionState::new("default", "session-test-2");
        let (mut writer, reader) = duplex(4096);
        let initialize = serde_json::to_string(&JsonRpcMessage::Request(JsonRpcRequest {
            id: RequestId::String("initialize".to_string()),
            method: "initialize".to_string(),
            params: Some(
                serde_json::to_value(TransportInitializeParams {
                    client_info: agent_protocol::TransportClientInfo {
                        name: "test-client".to_string(),
                        version: "0.0.0-test".to_string(),
                    },
                    capabilities: None,
                })
                .expect("serialize initialize params"),
            ),
        }))
        .expect("serialize initialize request");

        handle_transport_line(&initialize, &runtime, &mut session, &mut writer)
            .await
            .expect("initialize should succeed");
        assert!(!session.is_ready());

        let mut line = String::new();
        let mut reader = BufReader::new(reader);
        reader.read_line(&mut line).await.expect("read response");
        let JsonRpcMessage::Response(JsonRpcResponse { result, .. }) =
            serde_json::from_str(line.trim_end()).expect("parse response")
        else {
            panic!("expected initialize response");
        };
        let result: agent_protocol::TransportInitializeResult =
            serde_json::from_value(result).expect("decode initialize result");
        assert_eq!(result.transport, "remote");

        let initialized = serde_json::to_string(&JsonRpcMessage::Notification(
            agent_protocol::JsonRpcNotification {
                method: "initialized".to_string(),
                params: None,
            },
        ))
        .expect("serialize initialized notification");
        handle_transport_line(&initialized, &runtime, &mut session, &mut writer)
            .await
            .expect("initialized notification should succeed");
        assert!(session.is_ready());
    }

    #[tokio::test]
    async fn unsupported_request_after_initialize_returns_jsonrpc_error() {
        let runtime = test_runtime().await;
        let mut session = NodeSessionState::new("default", "session-test-3");
        let (mut writer, reader) = duplex(4096);

        let initialize = serde_json::to_string(&JsonRpcMessage::Request(JsonRpcRequest {
            id: RequestId::String("initialize".to_string()),
            method: "initialize".to_string(),
            params: Some(
                serde_json::to_value(TransportInitializeParams {
                    client_info: agent_protocol::TransportClientInfo {
                        name: "test-client".to_string(),
                        version: "0.0.0-test".to_string(),
                    },
                    capabilities: None,
                })
                .expect("serialize initialize params"),
            ),
        }))
        .expect("serialize initialize request");
        handle_transport_line(&initialize, &runtime, &mut session, &mut writer)
            .await
            .expect("initialize should succeed");

        let initialized = serde_json::to_string(&JsonRpcMessage::Notification(
            agent_protocol::JsonRpcNotification {
                method: "initialized".to_string(),
                params: None,
            },
        ))
        .expect("serialize initialized notification");
        handle_transport_line(&initialized, &runtime, &mut session, &mut writer)
            .await
            .expect("initialized should succeed");

        let unsupported = serde_json::to_string(&JsonRpcMessage::Request(JsonRpcRequest {
            id: RequestId::Integer(99),
            method: "conversation/unknown".to_string(),
            params: None,
        }))
        .expect("serialize unsupported request");
        handle_transport_line(&unsupported, &runtime, &mut session, &mut writer)
            .await
            .expect("unsupported request should return a protocol error");

        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .expect("read initialize response");
        line.clear();
        reader
            .read_line(&mut line)
            .await
            .expect("read unsupported error response");

        let JsonRpcMessage::Error(JsonRpcError { id, error }) =
            serde_json::from_str(line.trim_end()).expect("parse error response")
        else {
            panic!("expected jsonrpc error");
        };
        assert_eq!(id, RequestId::Integer(99));
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("unsupported request method"));
    }
}
