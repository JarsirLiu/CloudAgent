use crate::node::command_router::handle_command_line;
use crate::node::message_sync::write_node_event;
use crate::node::runtime::NodeRuntime;
use crate::node::session_state::NodeSessionState;
use crate::node::worker_manager::NodeEvent;
use anyhow::{Context, Result};
use std::ffi::OsString;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, BufReader};
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

    let listener = TcpListener::bind(&listen_address)
        .await
        .with_context(|| format!("failed to bind local node listener on {listen_address}"))?;
    tracing::info!("gatewayd local node listening on {listen_address}");
    let runtime = NodeRuntime::new(crate::node::worker_manager::WorkerManager::new(
        worker_program,
    ));

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        tracing::debug!("accepted local node client from {peer_addr}");
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let (reader, writer) = stream.into_split();
            if let Err(error) = run_connection(BufReader::new(reader), writer, runtime).await {
                tracing::warn!("local node connection failed: {error}");
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
                    match maybe_line.context("failed to read local node command line")? {
                        Some(line) => {
                            if !handle_command_line(
                                &line,
                                &runtime,
                                &mut session,
                                &mut writer,
                            )
                            .await? {
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
                                        "local node subscriber lagged; skipped {skipped} events"
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
                .context("failed to read local node command line")?
            {
                Some(line) => {
                    if !handle_command_line(&line, &runtime, &mut session, &mut writer).await? {
                        break;
                    }
                }
                None => break,
            }
        }
    }

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
    use super::arg_value;
    use std::ffi::OsString;

    #[test]
    fn parses_serve_flag_values() {
        let args = vec![
            OsString::from("--listen"),
            OsString::from("127.0.0.1:47070"),
            OsString::from("--worker-bin"),
            OsString::from("agentd.exe"),
        ];
        assert_eq!(
            arg_value(&args, "--listen"),
            Some(OsString::from("127.0.0.1:47070"))
        );
        assert_eq!(
            arg_value(&args, "--worker-bin"),
            Some(OsString::from("agentd.exe"))
        );
    }
}
