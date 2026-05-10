use agent_app_server_client::AppServerClient;
use agent_protocol::{
    NodeStatusResponse, PlatformConfigResponse, PlatformControlEntry,
    PlatformControlListResponse, PlatformControlStatusResponse,
};
use anyhow::{Result, bail};
use cli::agent_host::build_agent_host;
use cli::app::cli_settings::load_cli_settings;
use cli::terminal::apply_color_cli_preference;
use cli::transport::client::create_local_node_client;
use cli::{AppServerTarget, ConsoleBootstrap, ConsoleConfig, run_console};
use config::AgentConfig;
use infra_store::JsonConversationStore;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

enum RequestedConsoleTarget {
    Public(AppServerTarget),
    Embedded,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    cli::terminal::install_panic_hook();
    let raw_args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let args = normalize_cli_args(raw_args);
    if wants_help(&args) {
        print_help();
        return Ok(());
    }
    if wants_version(&args) {
        print_version();
        return Ok(());
    }
    apply_color_cli_preference(&args);
    ensure_user_config_exists()?;
    let workspace_root = std::env::current_dir()?;
    let config = if std::env::var("CLOUDAGENT_RELEASE_MODE").ok().as_deref() == Some("1") {
        AgentConfig::load_user_only(workspace_root)?
    } else {
        AgentConfig::load(workspace_root)?
    };
    let mut config = config;
    apply_data_dir_cli_override(&mut config, &args);
    if maybe_handle_node_command(&args, &config.runtime.data_root_dir).await? {
        return Ok(());
    }
    if maybe_handle_platform_command(&args, &config.runtime.data_root_dir).await? {
        return Ok(());
    }
    if let Ok(Some(settings)) = load_cli_settings(&config.runtime.conversation_store_dir) {
        config.cli.pre_llm_filter_enabled = settings.pre_llm_filter_enabled;
        config.cli.permission_mode = settings.permission_mode;
    }
    let allow_internal_targets = internal_targets_enabled();
    let requested_target = requested_console_target(&args, allow_internal_targets)?;
    let conversation_store_dir = config.runtime.conversation_store_dir.clone();
    let data_root_dir = config.runtime.data_root_dir.clone();
    let initial_filter_enabled = config.cli.pre_llm_filter_enabled;
    let initial_permission_mode = config.cli.permission_mode.clone();
    let conversation_id = resolve_initial_conversation_id(&args, &conversation_store_dir).await?;
    let runtime = if matches!(requested_target, RequestedConsoleTarget::Embedded) {
        let runtime = build_runtime(config)?;
        runtime.run_startup_retention_cleanup().await;
        Some(runtime)
    } else {
        None
    };
    let (target_label, bootstrap) = resolve_console_target(
        &args,
        &conversation_id,
        runtime.as_ref(),
        requested_target,
        &data_root_dir,
    )?;

    run_console(ConsoleConfig {
        conversation_id: conversation_id.clone(),
        workspace_root: std::env::current_dir()?,
        conversation_store_dir,
        initial_filter_enabled,
        initial_permission_mode,
        auto_approve: false,
        auto_approve_reason: None,
        target_label,
        bootstrap,
    })
    .await
}

async fn maybe_handle_platform_command(
    args: &[OsString],
    data_root_dir: &std::path::Path,
) -> Result<bool> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(false);
    };
    if command != "platform" {
        return Ok(false);
    }
    let client = create_platform_management_client(args, data_root_dir).await?;

    let action = args.get(1).and_then(|arg| arg.to_str()).unwrap_or("list");
    match action {
        "list" => {
            print_platform_list(&client).await?;
        }
        "status" => {
            if let Some(platform) = args.get(2).and_then(|arg| arg.to_str()) {
                print_platform_status(&client, platform).await?;
            } else {
                print_platform_list(&client).await?;
            }
        }
        "enable" => {
            let platform = args
                .get(2)
                .and_then(|arg| arg.to_str())
                .ok_or_else(|| anyhow::anyhow!("usage: cloudagent platform enable <platform>"))?;
            let response = client.set_platform_enabled_typed(platform, true).await?;
            println!("enabled platform `{platform}` via local node");
            print_single_platform(&response.platform);
        }
        "disable" => {
            let platform = args
                .get(2)
                .and_then(|arg| arg.to_str())
                .ok_or_else(|| anyhow::anyhow!("usage: cloudagent platform disable <platform>"))?;
            let response = client.set_platform_enabled_typed(platform, false).await?;
            println!("disabled platform `{platform}` via local node");
            print_single_platform(&response.platform);
        }
        "config" => {
            handle_platform_config_command(&client, args).await?;
        }
        other => bail!(
            "unknown platform action `{other}`. supported actions: list, status, enable, disable, config"
        ),
    }

    Ok(true)
}

async fn maybe_handle_node_command(
    args: &[OsString],
    data_root_dir: &std::path::Path,
) -> Result<bool> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Ok(false);
    };
    if command != "node" {
        return Ok(false);
    }
    let client = create_platform_management_client(args, data_root_dir).await?;
    let action = args.get(1).and_then(|arg| arg.to_str()).unwrap_or("status");
    match action {
        "status" => print_node_status(&client).await?,
        "stop" => {
            let response = client.stop_node_typed().await?;
            println!(
                "node stop requested: {}",
                if response.stopping { "accepted" } else { "ignored" }
            );
        }
        other => bail!("unknown node action `{other}`. supported actions: status, stop"),
    }
    Ok(true)
}

fn build_runtime(config: AgentConfig) -> Result<std::sync::Arc<agent_core::AgentHost>> {
    let runtime = match build_agent_host(config) {
        Ok(runtime) => runtime,
        Err(err) => {
            if err.to_string().contains("missing LLM api key") {
                let path = default_user_config_path()?;
                try_open_config_in_editor(&path);
                bail!(
                    "missing LLM api key. please edit {} and set llm.api_key",
                    path.display()
                );
            }
            return Err(err);
        }
    };
    Ok(runtime)
}

fn ensure_user_config_exists() -> Result<()> {
    let path = default_user_config_path()?;
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let template = r#"[llm]
base_url = "https://api.openai.com/v1"
api_key = "replace-with-your-api-key"
model = "gpt-4.1-mini"
temperature = 0.2
"#;
    fs::write(&path, template)?;
    eprintln!("created default config: {}", path.display());
    Ok(())
}

fn try_open_config_in_editor(path: &PathBuf) {
    if let Ok(editor) = std::env::var("EDITOR") {
        let _ = Command::new(editor).arg(path).status();
    } else {
        eprintln!("hint: set EDITOR (e.g. export EDITOR=vim) to auto-open config.");
    }
}

fn default_user_config_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("cannot resolve HOME/USERPROFILE"))?;
    Ok(home.join(".cloudagent").join("config.toml"))
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

fn normalize_cli_args(args: Vec<OsString>) -> Vec<OsString> {
    match args.first().and_then(|arg| arg.to_str()) {
        Some("start") => args.into_iter().skip(1).collect(),
        _ => args,
    }
}

fn apply_data_dir_cli_override(config: &mut AgentConfig, args: &[OsString]) {
    if let Some(value) = arg_value(args, "--data-dir") {
        let value = PathBuf::from(value);
        config.runtime.data_root_dir = if value.is_absolute() {
            value
        } else {
            config.workspace_root.join(value)
        };
        config.runtime.conversation_store_dir = config.runtime.data_root_dir.join("conversations");
        config.runtime.memory.root_dir = config.runtime.data_root_dir.join("state").join("memory");
    }
}

fn requested_target_name(args: &[OsString]) -> String {
    arg_value(args, "--target")
        .or_else(|| std::env::var_os("CLOUDAGENT_APP_SERVER_TARGET"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "local-node".to_string())
}

fn requested_console_target(
    args: &[OsString],
    allow_internal_targets: bool,
) -> Result<RequestedConsoleTarget> {
    let target = requested_target_name(args);

    match target.as_str() {
        "local-node" => Ok(RequestedConsoleTarget::Public(AppServerTarget::LocalNode)),
        "hub-node" => {
            let node_id = arg_value(args, "--hub-node-id")
                .or_else(|| std::env::var_os("CLOUDAGENT_HUB_NODE_ID"))
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| anyhow::anyhow!("hub-node target requires --hub-node-id"))?;
            Ok(RequestedConsoleTarget::Public(AppServerTarget::HubNode {
                node_id,
            }))
        }
        "embedded" if allow_internal_targets => Ok(RequestedConsoleTarget::Embedded),
        "embedded" => {
            bail!("target 'embedded' is internal-only and not part of the supported user path")
        }
        other => bail!("unknown target '{other}'. supported targets: local-node, hub-node"),
    }
}

async fn resolve_initial_conversation_id(
    args: &[OsString],
    conversation_store_dir: &std::path::Path,
) -> Result<String> {
    if let Some(conversation_id) =
        arg_value(args, "--conversation").and_then(|v| v.into_string().ok())
    {
        return Ok(conversation_id);
    }

    let store = JsonConversationStore::new(conversation_store_dir.to_path_buf());
    if let Some(conversation_id) = store.load_active_conversation().await?
        && !conversation_id.trim().is_empty()
    {
        return Ok(conversation_id);
    }

    let generated = format!(
        "draft-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| anyhow::anyhow!("system clock before unix epoch: {err}"))?
            .as_millis()
    );
    store.mark_active_conversation(&generated).await?;
    Ok(generated)
}

fn internal_targets_enabled() -> bool {
    std::env::var("CLOUDAGENT_INTERNAL_TARGETS").ok().as_deref() == Some("1")
}

fn resolve_console_target(
    args: &[OsString],
    _conversation_id: &str,
    runtime: Option<&std::sync::Arc<agent_core::AgentHost>>,
    requested_target: RequestedConsoleTarget,
    data_root_dir: &std::path::Path,
) -> Result<(String, ConsoleBootstrap)> {
    match requested_target {
        RequestedConsoleTarget::Public(AppServerTarget::LocalNode) => {
            let address = arg_value(args, "--node-addr")
                .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(default_node_addr);
            let (program, mut launch_args) = if let Some(program) = arg_value(args, "--node-bin")
                .or_else(|| std::env::var_os("CLOUDAGENT_NODE_BIN"))
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
            Ok((
                AppServerTarget::LocalNode.label().to_string(),
                ConsoleBootstrap::LocalNode {
                    address: address.clone(),
                    program,
                    args: launch_args,
                },
            ))
        }
        RequestedConsoleTarget::Public(AppServerTarget::HubNode { node_id: _ }) => {
            bail!("target 'hub-node' is reserved for hub mode and is not implemented yet");
        }
        RequestedConsoleTarget::Embedded => Ok((
            "embedded".to_string(),
            ConsoleBootstrap::Embedded {
                runtime: runtime
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("embedded target requires a local runtime"))?,
            },
        )),
    }
}

fn default_node_launcher() -> (OsString, Vec<OsString>) {
    if should_launch_gatewayd_via_cargo() {
        let target_dir = std::env::current_dir()
            .ok()
            .map(|dir| dir.join("target").join(".cloudagent-local-node"))
            .unwrap_or_else(|| PathBuf::from("target").join(".cloudagent-local-node"));
        return (
            OsString::from("cargo"),
            vec![
                OsString::from("run"),
                OsString::from("-p"),
                OsString::from("gatewayd"),
                OsString::from("--target-dir"),
                target_dir.into_os_string(),
                OsString::from("--"),
            ],
        );
    }

    (
        std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.parent()
                    .map(|parent| parent.join(exe_name("gatewayd")))
            })
            .map(|path| path.into_os_string())
            .unwrap_or_else(|| OsString::from(exe_name("gatewayd"))),
        Vec::new(),
    )
}

fn should_launch_gatewayd_via_cargo() -> bool {
    if std::env::var("CLOUDAGENT_RELEASE_MODE").ok().as_deref() == Some("1") {
        return false;
    }

    if std::env::var_os("CLOUDAGENT_NODE_BIN").is_some() {
        return false;
    }

    cfg!(debug_assertions) && std::env::current_dir().is_ok_and(|dir| dir.join("Cargo.toml").exists())
}

fn default_node_addr() -> String {
    if should_launch_gatewayd_via_cargo() {
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

fn wants_help(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == "--help" || arg == "-h")
}

fn wants_version(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == "--version" || arg == "-V")
}

fn print_help() {
    println!(
        "\
cloudagent cli

Usage:
  cloudagent platform list [--data-dir PATH]
  cloudagent platform status [PLATFORM] [--data-dir PATH]
  cloudagent platform enable PLATFORM [--data-dir PATH]
  cloudagent platform disable PLATFORM [--data-dir PATH]
  cloudagent platform config get PLATFORM [--data-dir PATH]
  cloudagent platform config set PLATFORM KEY VALUE [--data-dir PATH]
  cloudagent platform config clear PLATFORM KEY [--data-dir PATH]
  cloudagent node status [--data-dir PATH]
  cloudagent node stop [--data-dir PATH]
  cloudagent start [--target TARGET] [--node-bin PATH] [--node-addr ADDR] [--data-dir PATH] [--conversation ID] [--color WHEN] [--no-color]
  cloudagent [--target TARGET] [--node-bin PATH] [--node-addr ADDR] [--data-dir PATH] [--conversation ID] [--color WHEN] [--no-color]
  cloudagent --help
  cloudagent --version

Options:
  -h, --help                 Show this help text
      -V, --version              Show the CLI version
      platform                  Manage IM platform desired state for the resident node
      node                      Manage resident node lifecycle (status / stop)
      --target TARGET        Target selection: local-node (default) or hub-node (reserved)
      --node-bin PATH        node binary path when using local-node
      --node-addr ADDR       node listen address when using local-node
      --data-dir PATH        app data root dir for conversations, logs, and memory
      --hub-node-id ID       target node id when using the reserved hub-node target
      --conversation ID      Conversation id passed to the selected target
      --color WHEN           Color output: auto, always, or never
      --no-color             Disable color output
"
    );
}

fn print_version() {
    println!("{}", display_version());
}

async fn print_platform_list(client: &AppServerClient) -> Result<()> {
    let response: PlatformControlListResponse = client.request_platform_list_typed().await?;
    for entry in response.platforms {
        print_single_platform(&entry);
    }
    Ok(())
}

async fn print_platform_status(client: &AppServerClient, platform: &str) -> Result<()> {
    let response: PlatformControlStatusResponse =
        client.request_platform_status_typed(platform).await?;
    print_single_platform(&response.platform);
    Ok(())
}

fn print_single_platform(entry: &PlatformControlEntry) {
    let enabled = if entry.enabled { "enabled" } else { "disabled" };
    println!(
        "- {}: {enabled}, managed_by={}, updated_at_ms={}",
        entry.platform, entry.managed_by, entry.updated_at_ms
    );
}

async fn handle_platform_config_command(client: &AppServerClient, args: &[OsString]) -> Result<()> {
    let action = args.get(2).and_then(|arg| arg.to_str()).unwrap_or("get");
    match action {
        "get" => {
            let platform = args.get(3).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config get <platform>")
            })?;
            let response = client.request_platform_config_typed(platform).await?;
            print_platform_config(&response);
        }
        "set" => {
            let platform = args.get(3).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config set <platform> <key> <value>")
            })?;
            let key = args.get(4).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config set <platform> <key> <value>")
            })?;
            let value = args.get(5).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config set <platform> <key> <value>")
            })?;
            let response = client
                .set_platform_config_value_typed(platform, key, value)
                .await?;
            print_platform_config(&response);
        }
        "clear" => {
            let platform = args.get(3).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config clear <platform> <key>")
            })?;
            let key = args.get(4).and_then(|arg| arg.to_str()).ok_or_else(|| {
                anyhow::anyhow!("usage: cloudagent platform config clear <platform> <key>")
            })?;
            let response = client
                .clear_platform_config_value_typed(platform, key)
                .await?;
            print_platform_config(&response);
        }
        other => {
            bail!("unknown platform config action `{other}`. supported actions: get, set, clear")
        }
    }
    Ok(())
}

fn print_platform_config(response: &PlatformConfigResponse) {
    println!(
        "platform `{}`: {}",
        response.platform,
        if response.configured {
            "configured"
        } else {
            "incomplete"
        }
    );
    for field in &response.fields {
        let value = field.value.clone().unwrap_or_else(|| "<unset>".to_string());
        let required = if field.required {
            "required"
        } else {
            "optional"
        };
        let secret = if field.is_secret { ", secret" } else { "" };
        println!("- {}: {} ({required}{secret})", field.key, value);
    }
}

async fn create_platform_management_client(
    args: &[OsString],
    data_root_dir: &std::path::Path,
) -> Result<AppServerClient> {
    let address = arg_value(args, "--node-addr")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(default_node_addr);
    let (program, mut node_args) = if let Some(program) = arg_value(args, "--node-bin")
        .or_else(|| std::env::var_os("CLOUDAGENT_NODE_BIN"))
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
    create_local_node_client(&address, &program, &node_args).await
}

async fn print_node_status(client: &AppServerClient) -> Result<()> {
    let response: NodeStatusResponse = client.request_node_status_typed().await?;
    println!("listen_address: {}", response.listen_address);
    println!(
        "worker: {}",
        if response.worker_running {
            "running"
        } else {
            "idle"
        }
    );
    println!(
        "platform_runtimes: {}/{}",
        response.platform_runtime_count, response.managed_platform_count
    );
    Ok(())
}

fn display_version() -> &'static str {
    option_env!("CLOUDAGENT_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::{
        RequestedConsoleTarget, apply_data_dir_cli_override, normalize_cli_args,
        requested_console_target, resolve_console_target,
    };
    use cli::{AppServerTarget, ConsoleBootstrap};
    use config::AgentConfig;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn start_subcommand_is_treated_as_default_launch() {
        let args = vec![
            OsString::from("start"),
            OsString::from("--color"),
            OsString::from("always"),
        ];
        let normalized = normalize_cli_args(args);
        assert_eq!(
            normalized,
            vec![OsString::from("--color"), OsString::from("always")]
        );
    }

    #[test]
    fn non_start_args_are_left_unchanged() {
        let args = vec![OsString::from("--version")];
        let normalized = normalize_cli_args(args.clone());
        assert_eq!(normalized, args);
    }

    #[test]
    fn data_dir_cli_override_updates_data_bound_paths() {
        let workspace = PathBuf::from("D:\\learn\\gifti\\cloudagent");
        let mut config = AgentConfig::load(workspace.clone()).expect("load config");

        apply_data_dir_cli_override(
            &mut config,
            &[
                OsString::from("--data-dir"),
                OsString::from(".cloudagent-dev"),
            ],
        );

        assert_eq!(
            config.runtime.data_root_dir,
            workspace.join(".cloudagent-dev")
        );
        assert_eq!(
            config.runtime.conversation_store_dir,
            workspace.join(".cloudagent-dev").join("conversations")
        );
        assert_eq!(
            config.runtime.memory.root_dir,
            workspace
                .join(".cloudagent-dev")
                .join("state")
                .join("memory")
        );
    }

    #[tokio::test]
    async fn initial_conversation_id_uses_store_active_conversation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = infra_store::JsonConversationStore::new(temp.path().to_path_buf());
        store
            .mark_active_conversation("conversation-1")
            .await
            .expect("mark active");

        let conversation_id = super::resolve_initial_conversation_id(&[], temp.path())
            .await
            .expect("resolve initial conversation");

        assert_eq!(conversation_id, "conversation-1");
    }

    #[tokio::test]
    async fn initial_conversation_id_creates_draft_when_store_is_empty() {
        let temp = tempfile::tempdir().expect("tempdir");

        let conversation_id = super::resolve_initial_conversation_id(&[], temp.path())
            .await
            .expect("resolve initial conversation");

        assert!(conversation_id.starts_with("draft-"));
    }

    #[test]
    fn local_node_target_maps_to_node_bootstrap() {
        let args = vec![OsString::from("--target"), OsString::from("local-node")];
        let requested = requested_console_target(&args, false).expect("requested target");
        let expected_address = super::default_node_addr();
        let (target_label, bootstrap) = resolve_console_target(
            &args,
            "local-conversation",
            None,
            requested,
            PathBuf::from("D:\\learn\\gifti\\cloudagent\\data").as_path(),
        )
        .expect("local-node target should resolve");

        assert_eq!(target_label, "local-node");
        match bootstrap {
            ConsoleBootstrap::LocalNode {
                address,
                program,
                args,
            } => {
                assert_eq!(address, expected_address);
                let program_display = program.to_string_lossy();
                assert!(
                    program_display.contains("gatewayd") || program_display == "cargo",
                    "unexpected launcher: {program_display}"
                );
                let expected_args = if program_display == "cargo" {
                    let expected_target_dir = std::env::current_dir()
                        .expect("current dir")
                        .join("target")
                        .join(".cloudagent-local-node");
                    vec![
                        OsString::from("run"),
                        OsString::from("-p"),
                        OsString::from("gatewayd"),
                        OsString::from("--target-dir"),
                        expected_target_dir.into_os_string(),
                        OsString::from("--"),
                        OsString::from("serve"),
                        OsString::from("--listen"),
                        OsString::from(expected_address.as_str()),
                        OsString::from("--data-dir"),
                        OsString::from("D:\\learn\\gifti\\cloudagent\\data"),
                    ]
                } else {
                    vec![
                        OsString::from("serve"),
                        OsString::from("--listen"),
                        OsString::from(expected_address.as_str()),
                        OsString::from("--data-dir"),
                        OsString::from("D:\\learn\\gifti\\cloudagent\\data"),
                    ]
                };
                assert_eq!(
                    args,
                    expected_args
                );
            }
            other => panic!(
                "unexpected bootstrap: {}",
                std::any::type_name_of_val(&other)
            ),
        }
    }

    #[test]
    fn hub_node_target_is_reserved() {
        let args = vec![
            OsString::from("--target"),
            OsString::from("hub-node"),
            OsString::from("--hub-node-id"),
            OsString::from("node-a"),
        ];
        let requested = requested_console_target(&args, false).expect("requested target");
        match resolve_console_target(
            &args,
            "local-conversation",
            None,
            requested,
            PathBuf::from("D:\\learn\\gifti\\cloudagent\\data").as_path(),
        ) {
            Ok(_) => panic!("hub-node target should stay reserved"),
            Err(err) => assert!(
                err.to_string()
                    .contains("reserved for hub mode and is not implemented yet")
            ),
        }
    }

    #[test]
    fn embedded_target_is_rejected_without_internal_flag() {
        let args = vec![OsString::from("--target"), OsString::from("embedded")];
        match requested_console_target(&args, false) {
            Ok(_) => panic!("embedded should be internal-only"),
            Err(err) => assert!(err.to_string().contains("internal-only")),
        }
    }

    #[test]
    fn public_target_parser_only_exposes_local_and_hub() {
        let local = requested_console_target(
            &[OsString::from("--target"), OsString::from("local-node")],
            false,
        )
        .expect("local-node target");
        assert!(matches!(
            local,
            RequestedConsoleTarget::Public(AppServerTarget::LocalNode)
        ));

        let hub = requested_console_target(
            &[
                OsString::from("--target"),
                OsString::from("hub-node"),
                OsString::from("--hub-node-id"),
                OsString::from("node-a"),
            ],
            false,
        )
        .expect("hub-node target");
        assert!(matches!(
            hub,
            RequestedConsoleTarget::Public(AppServerTarget::HubNode { ref node_id })
            if node_id == "node-a"
        ));
    }

    #[test]
    fn display_version_prefers_build_metadata_when_available() {
        let expected = option_env!("CLOUDAGENT_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
        assert_eq!(super::display_version(), expected);
    }
}
