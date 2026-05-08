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
        .unwrap_or_else(|| "embedded".to_string());

    match target.as_str() {
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
        "local-node" => bail!(
            "target 'local-node' is reserved for the direct-mode migration and is not implemented yet"
        ),
        other => bail!(
            "unknown target '{other}'. supported targets in this migration stage: embedded, worker-stdio, local-node"
        ),
    }
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
  cloudagent start [--target TARGET] [--app-server-bin PATH] [--conversation ID] [--color WHEN] [--no-color]
  cloudagent [--target TARGET] [--app-server-bin PATH] [--conversation ID] [--color WHEN] [--no-color]
  cloudagent --help
  cloudagent --version

Options:
  -h, --help                 Show this help text
  -V, --version              Show the CLI version
      --target TARGET        Target selection: embedded (temporary), worker-stdio (temporary), or local-node (reserved)
      --app-server-bin PATH  worker binary path when using worker-stdio
      --conversation ID      Conversation id for worker-stdio
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
    fn local_node_target_is_reserved_until_node_exists() {
        let args = vec![OsString::from("--target"), OsString::from("local-node")];
        match resolve_console_target(&args, "local-conversation", None) {
            Ok(_) => panic!("local-node should not resolve before node exists"),
            Err(error) => assert!(
                error
                    .to_string()
                    .contains("target 'local-node' is reserved")
            ),
        }
    }
}
