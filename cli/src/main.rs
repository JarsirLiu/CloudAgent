use anyhow::{Result, bail};
use cli::agent_host::build_agent_host;
use cli::app::cli_settings::load_cli_settings;
use cli::{ConsoleConfig, ConsoleConnection, run_console};
use config::AgentConfig;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    cli::terminal::install_panic_hook();

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
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    let transport = arg_value(&args, "--transport")
        .or_else(|| std::env::var_os("CLOUDAGENT_CLIENT_TRANSPORT"))
        .and_then(|value| value.into_string().ok());

    let connection = if transport.as_deref() == Some("stdio") {
        let program = arg_value(&args, "--app-server-bin")
            .or_else(|| std::env::var_os("CLOUDAGENT_APP_SERVER_BIN"))
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|path| path.parent().map(|parent| parent.join(exe_name("agentd"))))
                    .map(|path| path.into_os_string())
                    .unwrap_or_else(|| OsString::from(exe_name("agentd")))
            });
        let remote_conversation = arg_value(&args, "--conversation")
            .and_then(|value| value.into_string().ok())
            .unwrap_or_else(|| conversation_id.clone());
        ConsoleConnection::Stdio {
            program,
            args: vec![
                OsString::from("app-server-stdio"),
                OsString::from("--conversation"),
                OsString::from(remote_conversation),
            ],
        }
    } else {
        ConsoleConnection::InProcess {
            runtime: runtime.clone(),
        }
    };

    run_console(ConsoleConfig {
        conversation_id: conversation_id.clone(),
        workspace_root: std::env::current_dir()?,
        conversation_store_dir: runtime.conversation_store_dir().to_path_buf(),
        initial_filter_enabled: runtime.cli_pre_llm_filter_enabled(),
        initial_permission_mode: runtime.cli_permission_mode().to_string(),
        auto_approve: false,
        auto_approve_reason: None,
        connection,
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
