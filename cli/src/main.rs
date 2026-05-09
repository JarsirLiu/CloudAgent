use anyhow::{Result, bail};
use cli::agent_host::build_agent_host;
use cli::app::cli_settings::load_cli_settings;
use cli::terminal::apply_color_cli_preference;
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
    if let Ok(Some(settings)) = load_cli_settings(&config.runtime.conversation_store_dir) {
        config.cli.pre_llm_filter_enabled = settings.pre_llm_filter_enabled;
        config.cli.permission_mode = settings.permission_mode;
    }
    let allow_internal_targets = internal_targets_enabled();
    let requested_target = requested_console_target(&args, allow_internal_targets)?;
    let conversation_store_dir = config.runtime.conversation_store_dir.clone();
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
    let (target_label, bootstrap) =
        resolve_console_target(&args, &conversation_id, runtime.as_ref(), requested_target)?;

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
) -> Result<(String, ConsoleBootstrap)> {
    match requested_target {
        RequestedConsoleTarget::Public(AppServerTarget::LocalNode) => {
            let address = arg_value(args, "--node-addr")
                .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| default_node_addr().to_string());
            let program = arg_value(args, "--node-bin")
                .or_else(|| std::env::var_os("CLOUDAGENT_NODE_BIN"))
                .unwrap_or_else(default_node_bin);
            Ok((
                AppServerTarget::LocalNode.label().to_string(),
                ConsoleBootstrap::LocalNode {
                    address: address.clone(),
                    program,
                    args: vec![
                        OsString::from("serve"),
                        OsString::from("--listen"),
                        OsString::from(address),
                    ],
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

fn default_node_bin() -> OsString {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|parent| parent.join(exe_name("gatewayd")))
        })
        .map(|path| path.into_os_string())
        .unwrap_or_else(|| OsString::from(exe_name("gatewayd")))
}

fn default_node_addr() -> &'static str {
    "127.0.0.1:47070"
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
  cloudagent start [--target TARGET] [--node-bin PATH] [--node-addr ADDR] [--conversation ID] [--color WHEN] [--no-color]
  cloudagent [--target TARGET] [--node-bin PATH] [--node-addr ADDR] [--conversation ID] [--color WHEN] [--no-color]
  cloudagent --help
  cloudagent --version

Options:
  -h, --help                 Show this help text
      -V, --version              Show the CLI version
      --target TARGET        Target selection: local-node (default) or hub-node (reserved)
      --node-bin PATH        node binary path when using local-node
      --node-addr ADDR       node listen address when using local-node
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

fn display_version() -> &'static str {
    option_env!("CLOUDAGENT_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::{
        RequestedConsoleTarget, normalize_cli_args, requested_console_target,
        resolve_console_target,
    };
    use cli::{AppServerTarget, ConsoleBootstrap};
    use std::ffi::OsString;

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
        let (target_label, bootstrap) =
            resolve_console_target(&args, "local-conversation", None, requested)
                .expect("local-node target should resolve");

        assert_eq!(target_label, "local-node");
        match bootstrap {
            ConsoleBootstrap::LocalNode {
                address,
                program,
                args,
            } => {
                assert_eq!(address, "127.0.0.1:47070");
                assert!(program.to_string_lossy().contains("gatewayd"));
                assert_eq!(
                    args,
                    vec![
                        OsString::from("serve"),
                        OsString::from("--listen"),
                        OsString::from("127.0.0.1:47070"),
                    ]
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
        match resolve_console_target(&args, "local-conversation", None, requested) {
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
