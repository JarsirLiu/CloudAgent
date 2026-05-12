use crate::{AppServerTarget, ConsoleBootstrap};
use agent_app_server_client::AppServerClient;
use anyhow::Result;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn arg_value(args: &[OsString], name: &str) -> Option<OsString> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

pub fn build_local_node_bootstrap(
    args: &[OsString],
    data_root_dir: &Path,
) -> (String, ConsoleBootstrap) {
    let address = arg_value(args, "--node-addr")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(default_node_addr);
    let (program, mut launch_args) = if let Some(program) =
        arg_value(args, "--node-bin").or_else(|| std::env::var_os("CLOUDAGENT_NODE_BIN"))
    {
        (program, Vec::new())
    } else {
        default_node_launcher()
    };
    launch_args.extend([
        OsString::from("serve"),
        OsString::from("--listen"),
        OsString::from(address.clone()),
        OsString::from("--data-dir"),
        data_root_dir.as_os_str().to_os_string(),
    ]);
    (
        AppServerTarget::LocalNode.label().to_string(),
        ConsoleBootstrap::LocalNode {
            address,
            program,
            args: launch_args,
            expected_data_root_dir: data_root_dir.to_path_buf(),
        },
    )
}

pub async fn create_node_management_client(
    args: &[OsString],
    data_root_dir: &Path,
) -> Result<AppServerClient> {
    let address = arg_value(args, "--node-addr")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(default_node_addr);
    let (program, mut node_args) = if let Some(program) =
        arg_value(args, "--node-bin").or_else(|| std::env::var_os("CLOUDAGENT_NODE_BIN"))
    {
        (program, Vec::new())
    } else {
        default_node_launcher()
    };
    node_args.extend([
        OsString::from("serve"),
        OsString::from("--listen"),
        OsString::from(address.clone()),
        OsString::from("--data-dir"),
        data_root_dir.as_os_str().to_os_string(),
    ]);
    crate::transport::client::create_local_node_client(
        &address,
        &program,
        &node_args,
        data_root_dir,
    )
    .await
}

pub async fn connect_node_management_client(
    args: &[OsString],
    data_root_dir: &Path,
) -> Result<AppServerClient> {
    let address = arg_value(args, "--node-addr")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(default_node_addr);
    crate::transport::client::connect_existing_local_node_client(&address, data_root_dir).await
}

pub fn default_node_launcher() -> (OsString, Vec<OsString>) {
    if should_launch_node_via_cargo() {
        let target_dir = std::env::current_dir()
            .ok()
            .map(|dir| dir.join("target").join(".cloudagent-local-node"))
            .unwrap_or_else(|| PathBuf::from("target").join(".cloudagent-local-node"));
        return (
            OsString::from("cargo"),
            vec![
                OsString::from("run"),
                OsString::from("-p"),
                OsString::from("node"),
                OsString::from("--target-dir"),
                target_dir.into_os_string(),
                OsString::from("--"),
            ],
        );
    }

    (
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join(exe_name("node"))))
            .map(|path| path.into_os_string())
            .unwrap_or_else(|| OsString::from(exe_name("node"))),
        Vec::new(),
    )
}

pub fn should_launch_node_via_cargo() -> bool {
    if config::release_mode_enabled() {
        return false;
    }

    if std::env::var_os("CLOUDAGENT_NODE_BIN").is_some() {
        return false;
    }

    cfg!(debug_assertions)
        && std::env::current_dir().is_ok_and(|dir| dir.join("Cargo.toml").exists())
}

pub fn default_node_addr() -> String {
    if should_launch_node_via_cargo() {
        return format!("127.0.0.1:{}", workspace_scoped_node_port());
    }
    "127.0.0.1:47070".to_string()
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

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}
