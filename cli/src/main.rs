use anyhow::Result;
use cli::agent_host::build_agent_host;
use cli::console_entry::{apply_data_dir_cli_override, run_console_surface};
use cli::terminal::apply_color_cli_preference;
use config::AgentConfig;
use std::ffi::OsString;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    cli::terminal::install_panic_hook();
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
    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load_runtime(workspace_root)?;
    let mut config = config;
    apply_data_dir_cli_override(&mut config, &args);
    run_console_surface(&args, config, build_runtime).await
}

fn build_runtime(config: AgentConfig) -> Result<std::sync::Arc<agent_core::AgentHost>> {
    build_agent_host(config)
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
cli

Usage:
  cli [--target TARGET] [--node-bin PATH] [--node-addr ADDR] [--data-dir PATH] [--conversation ID] [--color WHEN] [--no-color]
  cli --help
  cli --version
"
    );
}

fn print_version() {
    println!(
        "{}",
        option_env!("CLOUDAGENT_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
    );
}
