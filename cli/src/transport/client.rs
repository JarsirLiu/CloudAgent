use crate::app::{ConsoleBootstrap, ConsoleConfig};
use agent_app_server_client::{
    AppServerClient, AppServerConnectInfo, InProcessClientConfig, RemoteClientConfig,
};
use anyhow::{Result, anyhow};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::time::Instant;

pub(crate) async fn create_client(
    config: &ConsoleConfig,
    conversation_id: String,
) -> Result<AppServerClient> {
    match &config.bootstrap {
        ConsoleBootstrap::LocalNode {
            address,
            program,
            args,
        } => create_local_node_client(address, program, args).await,
        ConsoleBootstrap::Embedded { runtime } => {
            Ok(AppServerClient::in_process(InProcessClientConfig {
                runtime: runtime.clone(),
                conversation_id,
                auto_approve: config.auto_approve,
                auto_approve_reason: config.auto_approve_reason.clone(),
            }))
        }
    }
}

pub async fn create_local_node_client(
    address: &str,
    program: &std::ffi::OsString,
    args: &[std::ffi::OsString],
) -> Result<AppServerClient> {
    match connect_local_node_once(address).await {
        Ok(client) => Ok(client),
        Err(first_error) => {
            if existing_node_looks_unhealthy(&first_error) {
                return Err(anyhow!(
                    "failed to connect to local node at {address}: {first_error}; an existing local node is already listening but did not complete the transport handshake. stop the stale `gatewayd` process and retry"
                ));
            }
            let mut child = spawn_local_node_process(program, args)?;
            wait_for_service(
                || connect_local_node_once(address),
                Some(&mut child),
                local_node_launch_timeout(program, args),
                Duration::from_millis(100),
            )
            .await
            .map_err(|wait_error| {
                anyhow!(
                    "failed to connect to local node at {address}; initial error: {first_error}; {wait_error}"
                )
            })
        }
    }
}

fn existing_node_looks_unhealthy(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("local node closed during initialize")
        || message.contains("local node initialize failed")
        || message.contains("timed out initializing local node")
}

fn local_node_launch_timeout(program: &std::ffi::OsString, args: &[std::ffi::OsString]) -> Duration {
    if launches_gatewayd_via_cargo(program, args) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(5)
    }
}

fn launches_gatewayd_via_cargo(program: &std::ffi::OsString, args: &[std::ffi::OsString]) -> bool {
    let program = program.to_string_lossy().to_ascii_lowercase();
    if !(program == "cargo" || program.ends_with("\\cargo.exe") || program.ends_with("/cargo")) {
        return false;
    }

    args.iter()
        .map(|arg| arg.to_string_lossy().to_ascii_lowercase())
        .take(4)
        .any(|arg| arg == "gatewayd")
}

fn spawn_local_node_process(
    program: &OsString,
    args: &[OsString],
) -> Result<std::process::Child> {
    if launches_gatewayd_via_cargo(program, args) {
        return spawn_workspace_built_local_node(program, args);
    }

    Ok(std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

fn spawn_workspace_built_local_node(
    program: &OsString,
    args: &[OsString],
) -> Result<std::process::Child> {
    let target_dir = parse_cargo_target_dir(args)
        .ok_or_else(|| anyhow!("missing --target-dir in cargo-based local node launcher"))?;
    let service_args = args
        .iter()
        .position(|arg| arg == "--")
        .map(|index| args[index + 1..].to_vec())
        .ok_or_else(|| anyhow!("missing `--` separator in cargo-based local node launcher"))?;

    let status = std::process::Command::new(program)
        .args([
            OsString::from("build"),
            OsString::from("-p"),
            OsString::from("gatewayd"),
            OsString::from("-p"),
            OsString::from("agentd"),
            OsString::from("--target-dir"),
            target_dir.clone().into_os_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(anyhow!(
            "failed to build local node toolchain via cargo (status: {status})"
        ));
    }

    let gatewayd_bin = target_dir.join(debug_exe_name("gatewayd"));
    let agentd_bin = target_dir.join(debug_exe_name("agentd"));
    let mut final_args = service_args;
    final_args.extend([
        OsString::from("--worker-bin"),
        agentd_bin.clone().into_os_string(),
    ]);

    Ok(std::process::Command::new(gatewayd_bin)
        .args(final_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

fn parse_cargo_target_dir(args: &[OsString]) -> Option<PathBuf> {
    args.windows(2)
        .find(|pair| pair[0] == "--target-dir")
        .map(|pair| PathBuf::from(&pair[1]))
}

fn debug_exe_name(base: &str) -> PathBuf {
    let exe = if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    };
    Path::new("debug").join(exe)
}

async fn connect_local_node_once(address: &str) -> Result<AppServerClient> {
    AppServerClient::remote(RemoteClientConfig {
        address: address.to_string(),
        client: AppServerConnectInfo {
            client_name: env!("CARGO_PKG_NAME").to_string(),
            client_version: option_env!("CLOUDAGENT_BUILD_VERSION")
                .unwrap_or(env!("CARGO_PKG_VERSION"))
                .to_string(),
            experimental_api: true,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: agent_app_server_client::DEFAULT_EVENT_CHANNEL_CAPACITY,
        },
        connect_timeout: Duration::from_secs(1),
        initialize_timeout: Duration::from_secs(5),
    })
    .await
}

async fn wait_for_service<T, F, Fut>(
    mut connect_once: F,
    mut child: Option<&mut std::process::Child>,
    timeout: Duration,
    retry_interval: Duration,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let deadline = Instant::now() + timeout;
    let mut last_error = None;

    loop {
        if let Some(process) = child.as_deref_mut()
            && let Some(status) = process.try_wait()?
        {
            let detail = last_error.unwrap_or_else(|| "service did not accept connections".into());
            return Err(anyhow!(
                "launched process exited before the service became reachable (status: {status}); last connection error: {detail}"
            ));
        }

        match connect_once().await {
            Ok(client) => return Ok(client),
            Err(error) => last_error = Some(error.to_string()),
        }

        if Instant::now() >= deadline {
            let detail = last_error.unwrap_or_else(|| "service did not accept connections".into());
            return Err(anyhow!(
                "service did not become reachable within {} ms; last connection error: {detail}",
                timeout.as_millis()
            ));
        }

        tokio::time::sleep(retry_interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{connect_local_node_once, existing_node_looks_unhealthy, wait_for_service};
    use agent_protocol::{
        ConversationHistoryResponse, ConversationStatusResponse, JsonRpcRequest, RequestId,
    };
    use anyhow::{Result, anyhow};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use std::{ffi::OsString, path::PathBuf};
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
    #[ignore = "manual smoke test: requires fresh prebuilt gatewayd/agentd binaries"]
    async fn local_node_remote_smoke_supports_startup_typed_reads() {
        let exe_dir = current_binary_dir();
        let gatewayd = exe_dir.join(exe_name("gatewayd"));
        let agentd = exe_dir.join(exe_name("agentd"));
        assert!(
            gatewayd.exists(),
            "missing gatewayd binary at {}",
            gatewayd.display()
        );
        assert!(
            agentd.exists(),
            "missing agentd binary at {}",
            agentd.display()
        );

        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
        let addr = probe.local_addr().expect("probe local addr");
        drop(probe);
        let address = addr.to_string();
        let mut child = std::process::Command::new(&gatewayd)
            .args([
                OsString::from("serve"),
                OsString::from("--listen"),
                OsString::from(&address),
                OsString::from("--worker-bin"),
                agentd.into_os_string(),
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn gatewayd");

        let mut client = wait_for_service(
            || connect_local_node_once(&address),
            Some(&mut child),
            Duration::from_secs(5),
            Duration::from_millis(100),
        )
        .await
        .expect("connect local node");

        let conversation_id = "smoke-startup";
        let history: ConversationHistoryResponse = tokio::time::timeout(
            Duration::from_secs(5),
            client.request_typed(JsonRpcRequest {
                id: RequestId::String("smoke-history".to_string()),
                method: "conversation/history".to_string(),
                params: Some(serde_json::json!({ "conversation_id": conversation_id })),
            }),
        )
        .await
        .expect("history request timed out")
        .expect("history response");
        assert!(history.turns.is_empty());

        let status: ConversationStatusResponse = tokio::time::timeout(
            Duration::from_secs(5),
            client.request_typed(JsonRpcRequest {
                id: RequestId::String("smoke-status".to_string()),
                method: "conversation/status".to_string(),
                params: Some(serde_json::json!({ "conversation_id": conversation_id })),
            }),
        )
        .await
        .expect("status request timed out")
        .expect("status response");
        assert_eq!(status.snapshot.conversation_id, conversation_id);
        assert!(
            client.try_next_event().is_none(),
            "startup typed reads should not enqueue duplicate history/status notifications"
        );

        client.shutdown().await.expect("shutdown client");
        let _ = child.kill();
        let _ = child.wait();
    }

    fn current_binary_dir() -> PathBuf {
        let exe = std::env::current_exe().expect("current exe");
        let exe_dir = exe.parent().expect("exe parent");
        if exe_dir.file_name().is_some_and(|name| name == "deps") {
            exe_dir.parent().expect("debug dir parent").to_path_buf()
        } else {
            exe_dir.to_path_buf()
        }
    }

    fn exe_name(base: &str) -> String {
        if cfg!(windows) {
            format!("{base}.exe")
        } else {
            base.to_string()
        }
    }
}
