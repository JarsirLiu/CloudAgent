use crate::impls::command::ShellCommandTool as ShellCommandDescriptorTool;
use crate::registry::shared::{LocalTool, ToolInvocationOutput, read_streaming_pipe, resolve_workspace_path};
use agent_core::{ToolExecutionContext, ToolOutputStream, ToolSpec};
use agent_protocol::{CommandExecutionStatus, StructuredToolResult};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

pub(crate) struct ShellCommandTool;

#[derive(Deserialize)]
struct ShellCommandArgs {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[async_trait]
impl LocalTool for ShellCommandTool {
    fn spec(&self) -> ToolSpec {
        ShellCommandDescriptorTool::descriptor().spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ShellCommandArgs = serde_json::from_value(arguments)?;
        let workdir = resolve_workspace_path(&ctx.workspace_root, args.workdir.as_deref())?;
        let timeout_ms = args.timeout_ms.unwrap_or(ctx.default_shell_timeout_ms).max(1_000);
        let started_at = Instant::now();

        let mut command = if cfg!(windows) {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoLogo").arg("-NoProfile").arg("-Command").arg(&args.command);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-lc").arg(&args.command);
            cmd
        };
        command.current_dir(&workdir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        command.kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("failed to capture command stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("failed to capture command stderr"))?;
        let stdout_task = tokio::spawn(read_streaming_pipe(stdout, ToolOutputStream::Stdout, ctx.output_tx.clone()));
        let stderr_task = tokio::spawn(read_streaming_pipe(stderr, ToolOutputStream::Stderr, ctx.output_tx.clone()));

        let status = tokio::select! {
            _ = ctx.cancellation_token.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                bail!("command aborted by user");
            }
            waited = timeout(Duration::from_millis(timeout_ms), child.wait()) => {
                match waited {
                    Ok(result) => result?,
                    Err(_) => {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        bail!("command timed out after {timeout_ms}ms");
                    }
                }
            }
        };

        let stdout = String::from_utf8_lossy(&stdout_task.await??).trim().to_string();
        let stderr = String::from_utf8_lossy(&stderr_task.await??).trim().to_string();
        let duration_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let exit_code = status.code().unwrap_or(-1);
        let current_directory = workdir.display().to_string();
        let content = format!(
            "command: {}\ncurrent_directory: {}\nexit_code: {}\nsuccess: {}\n\nstdout:\n{}\n\nstderr:\n{}",
            args.command,
            current_directory,
            exit_code,
            status.success(),
            if stdout.is_empty() { "(empty)" } else { &stdout },
            if stderr.is_empty() { "(empty)" } else { &stderr },
        );

        Ok(ToolInvocationOutput {
            content: content.clone(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: args.command,
                current_directory,
                status: if status.success() { CommandExecutionStatus::Completed } else { CommandExecutionStatus::Failed },
                exit_code: Some(exit_code),
                success: Some(status.success()),
                stdout: Some(stdout),
                stderr: Some(stderr),
                aggregated_output: Some(content),
                duration_ms: Some(duration_ms),
            }),
        })
    }
}
