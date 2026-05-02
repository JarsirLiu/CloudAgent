use crate::registry::shared::{
    LocalTool, ToolInvocationOutput, read_streaming_pipe, resolve_workspace_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use agent_core::{ToolExecutionContext, ToolOutputStream};
use agent_protocol::{CommandExecutionStatus, StructuredToolResult};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::env;
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

pub struct ShellCommandTool;

impl ShellCommandTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::CommandExecution,
            ToolRisk::High,
            vec!["explore", "edit", "verify", "general"],
            ToolSpec {
                name: "shell_command".to_string(),
                description: "Run a local shell command for build, test, git, or high-density workspace inspection.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "workdir": { "type": "string" },
                        "timeout_ms": { "type": "integer", "minimum": 1000 }
                    },
                    "required": ["command"]
                }),
                mutating: true,
                requires_approval: true,
                item_kind: agent_protocol::TurnItemKind::CommandExecution,
                delta_kind: agent_protocol::TurnItemDeltaKind::CommandExecutionOutput,
                approval_reason: Some("Shell commands can inspect or modify the workspace.".to_string()),
            },
        )
    }
}

#[derive(Deserialize)]
struct ShellCommandArgs {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

pub(crate) struct ShellCommandLocalTool;

#[async_trait]
impl LocalTool for ShellCommandLocalTool {
    fn spec(&self) -> ToolSpec {
        ShellCommandTool::descriptor().spec
    }

    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ShellCommandArgs = serde_json::from_value(arguments)?;
        let workdir = resolve_workspace_path(&ctx.workspace_root, args.workdir.as_deref())?;
        let timeout_ms = args
            .timeout_ms
            .unwrap_or(ctx.default_shell_timeout_ms)
            .max(1_000);
        let started_at = Instant::now();
        let mut command = if cfg!(windows) {
            let mut cmd = Command::new(preferred_windows_shell());
            cmd.arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(windows_utf8_command(&args.command));
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-lc").arg(&args.command);
            cmd
        };
        command
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.kill_on_drop(true);
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture command stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture command stderr"))?;
        let stdout_task = tokio::spawn(read_streaming_pipe(
            stdout,
            ToolOutputStream::Stdout,
            ctx.output_tx.clone(),
        ));
        let stderr_task = tokio::spawn(read_streaming_pipe(
            stderr,
            ToolOutputStream::Stderr,
            ctx.output_tx.clone(),
        ));
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
        let stdout = String::from_utf8_lossy(&stdout_task.await??)
            .trim()
            .to_string();
        let stderr = String::from_utf8_lossy(&stderr_task.await??)
            .trim()
            .to_string();
        let duration_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let exit_code = status.code().unwrap_or(-1);
        let current_directory = workdir.display().to_string();
        let content = format!(
            "command: {}\ncurrent_directory: {}\nexit_code: {}\nsuccess: {}\n\nstdout:\n{}\n\nstderr:\n{}",
            args.command,
            current_directory,
            exit_code,
            status.success(),
            if stdout.is_empty() {
                "(empty)"
            } else {
                &stdout
            },
            if stderr.is_empty() {
                "(empty)"
            } else {
                &stderr
            },
        );
        Ok(ToolInvocationOutput {
            content: content.clone(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: args.command,
                current_directory,
                status: if status.success() {
                    CommandExecutionStatus::Completed
                } else {
                    CommandExecutionStatus::Failed
                },
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

fn preferred_windows_shell() -> String {
    find_windows_shell().unwrap_or_else(|| "powershell".to_string())
}

fn find_windows_shell() -> Option<String> {
    for candidate in ["pwsh.exe", "pwsh", "powershell.exe", "powershell"] {
        if command_exists(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn command_exists(candidate: &str) -> bool {
    if candidate.contains('\\') || candidate.contains('/') {
        return std::path::Path::new(candidate).exists();
    }
    let path_value = env::var_os("PATH");
    let Some(path_value) = path_value else {
        return false;
    };
    env::split_paths(&path_value).any(|dir| {
        let direct = dir.join(candidate);
        if direct.exists() {
            return true;
        }
        if direct.extension().is_none() {
            return dir.join(format!("{candidate}.exe")).exists();
        }
        false
    })
}

fn windows_utf8_command(command: &str) -> String {
    format!(
        concat!(
            "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); ",
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); ",
            "$OutputEncoding = [System.Text.UTF8Encoding]::new($false); ",
            "chcp 65001 > $null; ",
            "{command}"
        ),
        command = command
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_exists_rejects_missing_binary() {
        assert!(!command_exists("cloudagent-definitely-missing-command"));
    }
}
