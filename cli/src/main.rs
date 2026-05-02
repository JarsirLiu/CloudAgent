use agent_runtime::AgentRuntime;
use anyhow::Result;
use cli::{ConsoleConfig, ConsoleConnection, run_console};
use config::AgentConfig;
use std::ffi::OsString;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    cli::terminal::install_panic_hook();

    let workspace_root = std::env::current_dir()?;
    let config = AgentConfig::load(workspace_root)?;
    let runtime = Arc::new(AgentRuntime::from_config(config)?);
    let conversation_id = runtime.default_conversation_id().to_string();
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
        ConsoleConnection::InProcess { runtime }
    };

    run_console(ConsoleConfig {
        conversation_id: conversation_id.clone(),
        workspace_root: std::env::current_dir()?,
        auto_approve: false,
        auto_approve_reason: None,
        connection,
    })
    .await
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
        .find(|pair| pair[0] == OsString::from(name))
        .map(|pair| pair[1].clone())
}
