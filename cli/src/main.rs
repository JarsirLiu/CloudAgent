use anyhow::{Result, bail};
use cli::agent_host::build_agent_host;
use cli::app::cli_settings::load_cli_settings;
use cli::terminal::apply_color_cli_preference;
use cli::{AppServerTarget, ConsoleBootstrap, ConsoleConfig, run_console};
use config::AgentConfig;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    runtime.run_startup_retention_cleanup().await;
    let conversation_id = runtime.ensure_active_conversation().await?;
    let (target, bootstrap) = resolve_console_target(&args, &conversation_id, Some(&runtime))?;

    run_console(ConsoleConfig {
        conversation_id: conversation_id.clone(),
        workspace_root: std::env::current_dir()?,
        conversation_store_dir: runtime.conversation_store_dir().to_path_buf(),
        initial_filter_enabled: runtime.cli_pre_llm_filter_enabled(),
        initial_permission_mode: runtime.cli_permission_mode().to_string(),
        auto_approve: false,
        auto_approve_reason: None,
        target,
        bootstrap,
    })
    .await
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

fn resolve_console_target(
    args: &[OsString],
    conversation_id: &str,
    runtime: Option<&std::sync::Arc<agent_core::AgentHost>>,
) -> Result<(AppServerTarget, ConsoleBootstrap)> {
    let target = arg_value(args, "--target")
        .or_else(|| std::env::var_os("CLOUDAGENT_APP_SERVER_TARGET"))
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "local-node".to_string());

    match target.as_str() {
        "local-node" => {
            let address = arg_value(args, "--node-addr")
                .or_else(|| std::env::var_os("CLOUDAGENT_NODE_ADDR"))
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| default_node_addr().to_string());
            let program = arg_value(args, "--node-bin")
                .or_else(|| std::env::var_os("CLOUDAGENT_NODE_BIN"))
                .unwrap_or_else(default_node_bin);
            Ok((
                AppServerTarget::LocalNode,
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
        "hub-node" => {
            let node_id = arg_value(args, "--hub-node-id")
                .or_else(|| std::env::var_os("CLOUDAGENT_HUB_NODE_ID"))
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| anyhow::anyhow!("hub-node target requires --hub-node-id"))?;
            let _target = AppServerTarget::HubNode { node_id };
            bail!("target 'hub-node' is reserved for hub mode and is not implemented yet");
        }
        "embedded" => Ok((
            AppServerTarget::Embedded,
            ConsoleBootstrap::Embedded {
                runtime: runtime
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("embedded target requires a local runtime"))?,
            },
        )),
        "worker-stdio" => {
            let program = arg_value(args, "--app-server-bin")
                .or_else(|| std::env::var_os("CLOUDAGENT_APP_SERVER_BIN"))
                .unwrap_or_else(|| {
                    std::env::current_exe()
                        .ok()
                        .and_then(|path| {
                            path.parent().map(|parent| parent.join(exe_name("agentd")))
                        })
                        .map(|path| path.into_os_string())
                        .unwrap_or_else(|| OsString::from(exe_name("agentd")))
                });
            let remote_conversation = arg_value(args, "--conversation")
                .and_then(|value| value.into_string().ok())
                .unwrap_or_else(|| conversation_id.to_string());
            Ok((
                AppServerTarget::WorkerStdio,
                ConsoleBootstrap::WorkerStdio {
                    program,
                    args: vec![
                        OsString::from("app-server-stdio"),
                        OsString::from("--conversation"),
                        OsString::from(remote_conversation),
                    ],
                },
            ))
        }
        other => bail!(
            "unknown target '{other}'. supported targets in this migration stage: embedded, worker-stdio, local-node, hub-node"
        ),
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
      --target TARGET        Target selection: local-node (default), hub-node (reserved), embedded (internal), or worker-stdio (internal)
      --node-bin PATH        node binary path when using local-node
      --node-addr ADDR       node listen address when using local-node
      --hub-node-id ID       target node id when using the reserved hub-node target
      --app-server-bin PATH  worker binary path when using worker-stdio
      --conversation ID      Conversation id passed to the selected target
      --color WHEN           Color output: auto, always, or never
      --no-color             Disable color output
"
    );
}

fn print_version() {
    println!("{}", env!("CARGO_PKG_VERSION"));
}

#[cfg(test)]
mod tests {
    use super::{normalize_cli_args, resolve_console_target};
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

    #[test]
    fn worker_stdio_target_maps_to_worker_bootstrap() {
        let args = vec![
            OsString::from("--target"),
            OsString::from("worker-stdio"),
            OsString::from("--app-server-bin"),
            OsString::from("custom-agentd.exe"),
            OsString::from("--conversation"),
            OsString::from("remote-conversation"),
        ];
        let (target, bootstrap) = resolve_console_target(&args, "local-conversation", None)
            .expect("worker-stdio target should resolve");

        assert!(matches!(target, AppServerTarget::WorkerStdio));
        match bootstrap {
            ConsoleBootstrap::WorkerStdio { program, args } => {
                assert_eq!(program, OsString::from("custom-agentd.exe"));
                assert_eq!(
                    args,
                    vec![
                        OsString::from("app-server-stdio"),
                        OsString::from("--conversation"),
                        OsString::from("remote-conversation"),
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
    fn local_node_target_maps_to_node_bootstrap() {
        let args = vec![OsString::from("--target"), OsString::from("local-node")];
        let (target, bootstrap) = resolve_console_target(&args, "local-conversation", None)
            .expect("local-node target should resolve");

        assert!(matches!(target, AppServerTarget::LocalNode));
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
        match resolve_console_target(&args, "local-conversation", None) {
            Ok(_) => panic!("hub-node target should stay reserved"),
            Err(err) => assert!(
                err.to_string()
                    .contains("reserved for hub mode and is not implemented yet")
            ),
        }
    }
}
