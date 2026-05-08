use anyhow::{Context, Result, anyhow, bail};
use std::ffi::OsString;
use std::process::Stdio;
use tokio::io::{self, AsyncWriteExt};
use tokio::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("local-app-server") => run_local_app_server(&args[1..]).await,
        _ => {
            tracing::info!(
                "gatewayd local node bootstrap ready: {}",
                agent_gateway::crate_name()
            );
            tracing::info!(
                "run `gatewayd local-app-server --conversation <id>` to proxy a worker app server"
            );
            Ok(())
        }
    }
}

async fn run_local_app_server(args: &[OsString]) -> Result<()> {
    let conversation_id = arg_value(args, "--conversation")
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| anyhow!("missing required --conversation for local-app-server"))?;
    let worker_program = arg_value(args, "--worker-bin")
        .or_else(|| std::env::var_os("CLOUDAGENT_WORKER_BIN"))
        .unwrap_or_else(default_worker_bin);

    let mut child = Command::new(&worker_program)
        .args(worker_stdio_args(&conversation_id))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to spawn worker {:?}", worker_program))?;

    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("local node worker missing stdin"))?;
    let mut child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("local node worker missing stdout"))?;
    let mut parent_stdin = io::stdin();
    let mut parent_stdout = io::stdout();

    let upstream = tokio::spawn(async move {
        let copied = io::copy(&mut parent_stdin, &mut child_stdin).await?;
        child_stdin.shutdown().await?;
        Result::<u64>::Ok(copied)
    });
    let downstream = tokio::spawn(async move {
        let copied = io::copy(&mut child_stdout, &mut parent_stdout).await?;
        parent_stdout.flush().await?;
        Result::<u64>::Ok(copied)
    });

    let upstream_result = upstream.await.context("stdin relay task failed")??;
    let downstream_result = downstream.await.context("stdout relay task failed")??;
    let status = child.wait().await?;
    if !status.success() {
        bail!(
            "local node worker exited with status {status} after relaying {upstream_result} bytes in and {downstream_result} bytes out"
        );
    }
    Ok(())
}

fn worker_stdio_args(conversation_id: &str) -> Vec<OsString> {
    vec![
        OsString::from("app-server-stdio"),
        OsString::from("--conversation"),
        OsString::from(conversation_id),
    ]
}

fn default_worker_bin() -> OsString {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(exe_name("agentd"))))
        .map(|path| path.into_os_string())
        .unwrap_or_else(|| OsString::from(exe_name("agentd")))
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

#[cfg(test)]
mod tests {
    use super::{arg_value, worker_stdio_args};
    use std::ffi::OsString;

    #[test]
    fn parses_local_app_server_flag_values() {
        let args = vec![
            OsString::from("--conversation"),
            OsString::from("conversation-1"),
            OsString::from("--worker-bin"),
            OsString::from("agentd.exe"),
        ];
        assert_eq!(
            arg_value(&args, "--conversation"),
            Some(OsString::from("conversation-1"))
        );
        assert_eq!(
            arg_value(&args, "--worker-bin"),
            Some(OsString::from("agentd.exe"))
        );
    }

    #[test]
    fn builds_worker_stdio_arguments() {
        assert_eq!(
            worker_stdio_args("conversation-42"),
            vec![
                OsString::from("app-server-stdio"),
                OsString::from("--conversation"),
                OsString::from("conversation-42"),
            ]
        );
    }
}
