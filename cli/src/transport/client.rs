use crate::app::{ConsoleBootstrap, ConsoleConfig};
use agent_app_server_client::{
    AppServerClient, AppServerConnectInfo, InProcessClientConfig, RemoteClientConfig,
};
use agent_protocol::NodeStatusResponse;
use anyhow::{Result, anyhow};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub(crate) async fn create_client(
    config: &ConsoleConfig,
    conversation_id: String,
) -> Result<AppServerClient> {
    match &config.bootstrap {
        ConsoleBootstrap::LocalNode {
            address,
            program,
            args,
            expected_data_root_dir,
        } => create_local_node_client(address, program, args, expected_data_root_dir).await,
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
    expected_data_root_dir: &Path,
) -> Result<AppServerClient> {
    if launches_node_via_cargo(program, args)
        && let Ok(client) = connect_local_node_once(address).await
    {
        let client = verify_local_node_data_root(client, address, expected_data_root_dir).await?;
        stop_existing_development_node(client, address).await?;
    }

    match connect_local_node_once(address).await {
        Ok(client) => verify_local_node_data_root(client, address, expected_data_root_dir).await,
        Err(first_error) => {
            if existing_node_looks_unhealthy(&first_error) {
                return Err(anyhow!(
                    "failed to connect to local node at {address}: {first_error}; an existing local node is already listening but did not complete the transport handshake. stop the stale `node` process and retry"
                ));
            }
            let mut child = spawn_local_node_process(program, args)?;
            let client = wait_for_service(
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
            })?;
            verify_local_node_data_root(client, address, expected_data_root_dir).await
        }
    }
}

async fn stop_existing_development_node(client: AppServerClient, address: &str) -> Result<()> {
    let _ = client.stop_node_typed().await.map_err(|error| {
        anyhow!("failed to stop existing development local node at {address}: {error}")
    })?;
    wait_for_local_node_to_stop(address, Duration::from_secs(5), Duration::from_millis(100)).await
}

async fn wait_for_local_node_to_stop(
    address: &str,
    timeout: Duration,
    retry_interval: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match connect_local_node_once(address).await {
            Ok(_) => {
                if Instant::now() >= deadline {
                    return Err(anyhow!(
                        "existing development local node at {address} did not stop within {} ms",
                        timeout.as_millis()
                    ));
                }
                tokio::time::sleep(retry_interval).await;
            }
            Err(_) => return Ok(()),
        }
    }
}

pub async fn connect_existing_local_node_client(
    address: &str,
    expected_data_root_dir: &Path,
) -> Result<AppServerClient> {
    let client = connect_local_node_once(address).await?;
    verify_local_node_data_root(client, address, expected_data_root_dir).await
}

async fn verify_local_node_data_root(
    client: AppServerClient,
    address: &str,
    expected_data_root_dir: &Path,
) -> Result<AppServerClient> {
    let status: NodeStatusResponse = client
        .request_node_status_typed()
        .await
        .map_err(|error| anyhow!("failed to read local node status at {address}: {error}"))?;
    let expected = normalize_path_for_compare(expected_data_root_dir);
    let actual = normalize_path_for_compare(Path::new(&status.data_root_dir));
    if actual == expected {
        return Ok(client);
    }
    Err(anyhow!(
        "local node at {address} is using a different data root (expected `{}`, got `{}`). stop the stale `node` and restart cloudagent so `/session` and IM conversations read the same store",
        expected_data_root_dir.display(),
        status.data_root_dir
    ))
}

fn normalize_path_for_compare(path: &Path) -> String {
    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    normalized
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn existing_node_looks_unhealthy(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("local node closed during initialize")
        || message.contains("local node initialize failed")
        || message.contains("timed out initializing local node")
}

fn local_node_launch_timeout(
    program: &std::ffi::OsString,
    args: &[std::ffi::OsString],
) -> Duration {
    if launches_node_via_cargo(program, args) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(5)
    }
}

fn launches_node_via_cargo(program: &std::ffi::OsString, args: &[std::ffi::OsString]) -> bool {
    let program = program.to_string_lossy().to_ascii_lowercase();
    if !(program == "cargo" || program.ends_with("\\cargo.exe") || program.ends_with("/cargo")) {
        return false;
    }

    args.iter()
        .map(|arg| arg.to_string_lossy().to_ascii_lowercase())
        .take(4)
        .any(|arg| arg == "node")
}

fn spawn_local_node_process(program: &OsString, args: &[OsString]) -> Result<std::process::Child> {
    if launches_node_via_cargo(program, args) {
        return spawn_workspace_built_local_node(program, args);
    }

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached_node_process(&mut command);
    Ok(command.spawn()?)
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

    eprintln!(
        "Building local development node toolchain in {} ...",
        target_dir.display()
    );
    let status = std::process::Command::new(program)
        .args([
            OsString::from("build"),
            OsString::from("-p"),
            OsString::from("node"),
            OsString::from("-p"),
            OsString::from("agentd"),
            OsString::from("--target-dir"),
            target_dir.clone().into_os_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        return Err(anyhow!(
            "failed to build local node toolchain via cargo (status: {status})"
        ));
    }
    eprintln!("Local development node toolchain is ready.");

    let node_bin = target_dir.join(debug_exe_name("node"));
    let agentd_bin = target_dir.join(debug_exe_name("agentd"));
    let mut final_args = service_args;
    final_args.extend([
        OsString::from("--worker-bin"),
        agentd_bin.clone().into_os_string(),
    ]);

    let mut command = Command::new(node_bin);
    command
        .args(final_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_detached_node_process(&mut command);
    Ok(command.spawn()?)
}

fn configure_detached_node_process(_command: &mut Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        // Keep the local node in its own process group without surfacing a new
        // console window to the user on Windows.
        _command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
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
    #[ignore = "manual smoke test: requires fresh prebuilt node/agentd binaries"]
    async fn local_node_remote_smoke_supports_startup_typed_reads() {
        let exe_dir = current_binary_dir();
        let node = exe_dir.join(exe_name("node"));
        let agentd = exe_dir.join(exe_name("agentd"));
        assert!(node.exists(), "missing node binary at {}", node.display());
        assert!(
            agentd.exists(),
            "missing agentd binary at {}",
            agentd.display()
        );

        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe listener");
        let addr = probe.local_addr().expect("probe local addr");
        drop(probe);
        let address = addr.to_string();
        let mut child = std::process::Command::new(&node)
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
            .expect("spawn node");

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
