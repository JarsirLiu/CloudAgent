use crate::node::command_router::handle_command_message;
use crate::node::message_sync::write_node_event;
use crate::node::runtime::NodeRuntime;
use crate::node::session_state::NodeSessionState;
use crate::node::worker_manager::NodeEvent;
use agent_protocol::{JsonRpcError, JsonRpcErrorPayload, JsonRpcMessage, JsonRpcNotification, JsonRpcResponse, TransportInitializeParams, TransportInitializeResult, TransportServerInfo};
use anyhow::{Context, Result};
use std::ffi::OsString;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::broadcast;

pub(crate) async fn run_resident_node(args: &[OsString]) -> Result<()> {
    let listen_address = arg_value(args, "--listen")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| default_listen_address().to_string());
    let worker_program = arg_value(args, "--worker-bin")
        .or_else(|| std::env::var_os("CLOUDAGENT_WORKER_BIN"))
        .unwrap_or_else(default_worker_bin);
    let data_root_dir = arg_value(args, "--data-dir")
        .or_else(|| std::env::var_os("CLOUDAGENT_DATA_ROOT_DIR"));

    let listener = TcpListener::bind(&listen_address)
        .await
        .with_context(|| format!("failed to bind gatewayd remote host on {listen_address}"))?;
    tracing::info!("gatewayd remote app-server host listening on {listen_address}");
    let runtime = NodeRuntime::new(crate::node::worker_manager::WorkerManager::new(
        worker_program,
        data_root_dir,
    ));

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        tracing::debug!("accepted remote app-server client from {peer_addr}");
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();
            if let Err(error) = run_connection(BufReader::new(reader), writer, runtime).await {
                tracing::warn!("gatewayd remote host connection failed: {error}");
            }
        });
    }
}

async fn run_connection<R, W>(reader: R, mut writer: W, runtime: NodeRuntime) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut input_lines = reader.lines();
    let mut session = NodeSessionState::new("default");

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
                            write_node_event(&mut writer, event, &runtime).await?
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
            let _: TransportInitializeParams = serde_json::from_value(params)
                .context("failed to decode remote app-server initialize params")?;
            session.mark_initialize_accepted();
            write_jsonrpc_message(
                writer,
                JsonRpcMessage::Response(JsonRpcResponse {
                    id: request.id,
                    result: serde_json::to_value(TransportInitializeResult {
                        server_info: TransportServerInfo {
                            name: "gatewayd".to_string(),
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

fn default_listen_address() -> &'static str {
    "127.0.0.1:47070"
}

fn default_worker_bin() -> OsString {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(exe_name("agentd"))))
        .map(|path| path.into_os_string())
        .unwrap_or_else(|| OsString::from(exe_name("agentd")))
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

#[cfg(test)]
mod tests {
    use super::{arg_value, handle_transport_line};
    use crate::node::runtime::NodeRuntime;
    use crate::node::session_state::NodeSessionState;
    use crate::node::worker_manager::WorkerManager;
    use agent_protocol::{
        JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, RequestId,
        TransportInitializeParams,
    };
    use std::ffi::OsString;
    use tokio::io::{AsyncBufReadExt, BufReader, duplex};

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
        let runtime = NodeRuntime::new(WorkerManager::new(OsString::from("agentd.exe"), None));
        let mut session = NodeSessionState::new("default");
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
        let runtime = NodeRuntime::new(WorkerManager::new(OsString::from("agentd.exe"), None));
        let mut session = NodeSessionState::new("default");
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
        let runtime = NodeRuntime::new(WorkerManager::new(OsString::from("agentd.exe"), None));
        let mut session = NodeSessionState::new("default");
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
