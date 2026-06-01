mod product;

use anyhow::{Result, bail};
use cli::agent_host::build_agent_host;
use cli::console_entry::{apply_data_dir_cli_override, run_console_surface};
use cli::terminal::apply_color_cli_preference;
use config::AgentConfig;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
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
    let config = if config::release_mode_enabled() {
        AgentConfig::load_user_only(workspace_root)?
    } else {
        AgentConfig::load(std::env::current_dir()?)?
    };
    let mut config = config;
    apply_data_dir_cli_override(&mut config, &args);
    if product::maybe_handle_command(&args, &config.runtime.data_root_dir).await? {
        return Ok(());
    }
    let args = normalize_console_args(args);
    run_console_surface(&args, config, build_runtime).await
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
# model_reasoning_effort = "medium"
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

fn normalize_console_args(args: Vec<OsString>) -> Vec<OsString> {
    match args.first().and_then(|arg| arg.to_str()) {
        Some("cli") => args.into_iter().skip(1).collect(),
        _ => args,
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
cloudagent

Usage:
  cloudagent start [--node-bin PATH] [--node-addr ADDR] [--data-dir PATH]
  cloudagent status [--node-addr ADDR] [--data-dir PATH]
  cloudagent stop [--node-addr ADDR] [--data-dir PATH]
  cloudagent cli [--target TARGET] [--node-bin PATH] [--node-addr ADDR] [--data-dir PATH] [--conversation ID] [--color WHEN] [--no-color]
  cloudagent platform list [--data-dir PATH]
  cloudagent platform status [PLATFORM] [--data-dir PATH]
  cloudagent platform enable PLATFORM [--data-dir PATH]
  cloudagent platform disable PLATFORM [--data-dir PATH]
  cloudagent platform config get PLATFORM [--data-dir PATH]
  cloudagent platform config set PLATFORM KEY VALUE [--data-dir PATH]
  cloudagent platform config clear PLATFORM KEY [--data-dir PATH]
  cloudagent node status [--data-dir PATH]
  cloudagent node stop [--data-dir PATH]
  cloudagent --help
  cloudagent --version
"
    );
}

fn print_version() {
    println!(
        "{}",
        option_env!("CLOUDAGENT_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
    );
}
