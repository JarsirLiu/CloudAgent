use agent_core::{
    ToolCall, ToolExecutionContext, ToolExecutor, ToolOutputDelta, ToolOutputStream, ToolResult,
    ToolSpec, context::ToolExecutionContext as CtxAlias,
};
use agent_protocol::{CommandExecutionStatus, StructuredToolResult, WriteFileStatus};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

pub fn crate_name() -> &'static str {
    "agent-tools"
}

#[async_trait]
trait LocalTool: Send + Sync {
    fn spec(&self) -> ToolSpec;

    async fn invoke(&self, arguments: Value, ctx: &CtxAlias) -> Result<ToolInvocationOutput>;
}

#[derive(Clone, Debug)]
struct ToolInvocationOutput {
    content: String,
    summary: String,
    structured: Option<StructuredToolResult>,
}

#[derive(Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn LocalTool>>,
}

impl ToolRegistry {
    pub fn new(max_read_chars: usize) -> Self {
        let mut tools: BTreeMap<String, Arc<dyn LocalTool>> = BTreeMap::new();
        register(&mut tools, ShellCommandTool);
        register(&mut tools, ListDirTool);
        register(&mut tools, ReadFileTool { max_read_chars });
        register(&mut tools, WriteFileTool);
        Self { tools }
    }
}

#[async_trait]
impl ToolExecutor for ToolRegistry {
    fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let call_name = call.name.clone();
        let call_args = call.arguments.clone();
        let Some(tool) = self.tools.get(&call.name) else {
            return Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: "Tool not found".to_string(),
                summary: "tool lookup failed".to_string(),
                is_error: true,
                structured: None,
            });
        };

        match tool.invoke(call.arguments, ctx).await {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: output.content,
                summary: output.summary,
                is_error: false,
                structured: output.structured,
            }),
            Err(err) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: format!("Tool execution failed: {err:#}"),
                summary: "tool execution failed".to_string(),
                is_error: true,
                structured: structured_failure_result(&call_name, &call_args),
            }),
        }
    }
}

fn register<T>(tools: &mut BTreeMap<String, Arc<dyn LocalTool>>, tool: T)
where
    T: LocalTool + 'static,
{
    tools.insert(tool.spec().name.clone(), Arc::new(tool));
}

fn structured_failure_result(tool_name: &str, arguments: &Value) -> Option<StructuredToolResult> {
    match tool_name {
        "shell_command" => {
            let command = arguments
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let current_directory = arguments
                .get("workdir")
                .and_then(|value| value.as_str())
                .unwrap_or(".")
                .to_string();
            Some(StructuredToolResult::CommandExecution {
                command,
                current_directory,
                status: CommandExecutionStatus::Failed,
                exit_code: None,
                success: Some(false),
                stdout: None,
                stderr: None,
                aggregated_output: None,
                duration_ms: None,
            })
        }
        "write_file" => {
            let path = arguments
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            Some(StructuredToolResult::WriteFile {
                path,
                bytes_written: 0,
                status: WriteFileStatus::Failed,
            })
        }
        _ => None,
    }
}

struct ShellCommandTool;

#[derive(Deserialize)]
struct ShellCommandArgs {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

async fn read_streaming_pipe<R>(
    mut reader: R,
    stream: ToolOutputStream,
    output_tx: Option<tokio::sync::mpsc::UnboundedSender<ToolOutputDelta>>,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut collected = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let chunk = buffer[..read].to_vec();
        collected.extend_from_slice(&chunk);
        if let Some(output_tx) = &output_tx {
            let _ = output_tx.send(ToolOutputDelta {
                stream: stream.clone(),
                chunk: String::from_utf8_lossy(&chunk).to_string(),
            });
        }
    }
    Ok(collected)
}

#[async_trait]
impl LocalTool for ShellCommandTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell_command".to_string(),
            description: "Run a shell command inside the workspace and return stdout, stderr, exit code, and working directory.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute." },
                    "workdir": { "type": "string", "description": "Optional relative working directory inside the workspace." },
                    "timeout_ms": { "type": "integer", "minimum": 1000, "description": "Optional timeout in milliseconds." }
                },
                "required": ["command"]
            }),
            mutating: true,
            requires_approval: true,
            item_kind: agent_protocol::TurnItemKind::CommandExecution,
            delta_kind: agent_protocol::TurnItemDeltaKind::CommandExecutionOutput,
            approval_reason: Some("Shell commands can inspect or modify the workspace.".to_string()),
        }
    }

    async fn invoke(&self, arguments: Value, ctx: &CtxAlias) -> Result<ToolInvocationOutput> {
        let args: ShellCommandArgs = serde_json::from_value(arguments)?;
        let workdir = resolve_workspace_path(&ctx.workspace_root, args.workdir.as_deref())?;
        let normalized_command = normalize_shell_command(&args.command);
        let timeout_ms = args
            .timeout_ms
            .unwrap_or(ctx.default_shell_timeout_ms)
            .max(1_000);
        let started_at = Instant::now();

        let mut command = if cfg!(windows) {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(&normalized_command);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-lc").arg(&normalized_command);
            cmd
        };
        command
            .current_dir(&workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture command stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture command stderr"))?;
        let stdout_tx = ctx.output_tx.clone();
        let stderr_tx = ctx.output_tx.clone();
        let stdout_task = tokio::spawn(read_streaming_pipe(
            stdout,
            ToolOutputStream::Stdout,
            stdout_tx,
        ));
        let stderr_task = tokio::spawn(read_streaming_pipe(
            stderr,
            ToolOutputStream::Stderr,
            stderr_tx,
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
        let content = format_shell_output(
            &args.command,
            &current_directory,
            exit_code,
            status.success(),
            &stdout,
            &stderr,
        );
        let aggregated_output = content.clone();
        let summary = shell_summary(
            &args.command,
            &current_directory,
            exit_code,
            status.success(),
            &stdout,
        );

        Ok(ToolInvocationOutput {
            content,
            summary,
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
                aggregated_output: Some(aggregated_output),
                duration_ms: Some(duration_ms),
            }),
        })
    }
}

fn normalize_shell_command(command: &str) -> String {
    if cfg!(windows) {
        let trimmed = command.trim();
        if trimmed.eq_ignore_ascii_case("pwd") {
            return "Get-Location | Select-Object -ExpandProperty Path".to_string();
        }
    }
    command.to_string()
}

fn format_shell_output(
    command: &str,
    current_directory: &str,
    exit_code: i32,
    success: bool,
    stdout: &str,
    stderr: &str,
) -> String {
    let stdout_block = if stdout.is_empty() { "(empty)" } else { stdout };
    let stderr_block = if stderr.is_empty() { "(empty)" } else { stderr };

    format!(
        "command: {command}\ncurrent_directory: {current_directory}\nexit_code: {exit_code}\nsuccess: {success}\n\nstdout:\n{stdout_block}\n\nstderr:\n{stderr_block}"
    )
}

fn shell_summary(
    command: &str,
    current_directory: &str,
    exit_code: i32,
    success: bool,
    stdout: &str,
) -> String {
    let trimmed = command.trim();
    if success
        && (trimmed.eq_ignore_ascii_case("pwd")
            || trimmed.eq_ignore_ascii_case("Get-Location")
            || trimmed.eq_ignore_ascii_case("Get-Location | Select-Object -ExpandProperty Path"))
    {
        let shown = if stdout.is_empty() {
            current_directory
        } else {
            stdout
        };
        return format!("current directory is {shown}");
    }

    format!("shell command finished with exit code {exit_code}")
}

struct ListDirTool;

#[derive(Deserialize)]
struct ListDirArgs {
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl LocalTool for ListDirTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_dir".to_string(),
            description: "List files and directories under a relative workspace path.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional relative directory path inside the workspace." }
                }
            }),
            mutating: false,
            requires_approval: false,
            item_kind: agent_protocol::TurnItemKind::ToolCall,
            delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
            approval_reason: None,
        }
    }

    async fn invoke(&self, arguments: Value, ctx: &CtxAlias) -> Result<ToolInvocationOutput> {
        let args: ListDirArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, args.path.as_deref())?;
        let mut entries = fs::read_dir(&path).await?;
        let mut items = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            items.push(json!({
                "name": entry.file_name().to_string_lossy().to_string(),
                "path": entry.path().display().to_string(),
                "kind": if metadata.is_dir() { "dir" } else { "file" },
                "size": metadata.len(),
            }));
        }

        items.sort_by(|left, right| {
            left["name"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["name"].as_str().unwrap_or_default())
        });

        Ok(ToolInvocationOutput {
            content: serde_json::to_string_pretty(&items)?,
            summary: format!("listed {} entries", items.len()),
            structured: Some(StructuredToolResult::ListDirectory {
                path: path.display().to_string(),
                entry_count: items.len(),
            }),
        })
    }
}

struct ReadFileTool {
    max_read_chars: usize,
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
    #[serde(default)]
    max_chars: Option<usize>,
}

#[async_trait]
impl LocalTool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_string(),
            description: "Read a text file from the workspace. Large outputs are truncated."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path inside the workspace." },
                    "max_chars": { "type": "integer", "minimum": 128, "description": "Optional response limit." }
                },
                "required": ["path"]
            }),
            mutating: false,
            requires_approval: false,
            item_kind: agent_protocol::TurnItemKind::ToolCall,
            delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
            approval_reason: None,
        }
    }

    async fn invoke(&self, arguments: Value, ctx: &CtxAlias) -> Result<ToolInvocationOutput> {
        let args: ReadFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let text = fs::read_to_string(&path).await?;
        let max_chars = args.max_chars.unwrap_or(self.max_read_chars).max(128);
        let content = if text.chars().count() > max_chars {
            let truncated: String = text.chars().take(max_chars).collect();
            format!("{truncated}\n\n[truncated]")
        } else {
            text
        };
        let char_count = content.chars().count();
        let truncated = content.ends_with("\n\n[truncated]");

        Ok(ToolInvocationOutput {
            content,
            summary: format!("read {}", path.display()),
            structured: Some(StructuredToolResult::ReadFile {
                path: path.display().to_string(),
                truncated,
                char_count,
            }),
        })
    }
}

struct WriteFileTool;

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

#[async_trait]
impl LocalTool for WriteFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".to_string(),
            description: "Write or replace a text file inside the workspace. Parent directories are created if needed.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative file path inside the workspace." },
                    "content": { "type": "string", "description": "The full file content to write." }
                },
                "required": ["path", "content"]
            }),
            mutating: true,
            requires_approval: true,
            item_kind: agent_protocol::TurnItemKind::FileChange,
            delta_kind: agent_protocol::TurnItemDeltaKind::FileChangeOutput,
            approval_reason: Some("Writing files modifies the workspace.".to_string()),
        }
    }

    async fn invoke(&self, arguments: Value, ctx: &CtxAlias) -> Result<ToolInvocationOutput> {
        let args: WriteFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let Some(parent) = path.parent() else {
            bail!("cannot determine parent directory for {}", path.display());
        };
        fs::create_dir_all(parent).await?;
        let bytes_written = args.content.len();
        fs::write(&path, args.content).await?;

        Ok(ToolInvocationOutput {
            content: format!("Wrote {}", path.display()),
            summary: format!("wrote {}", path.display()),
            structured: Some(StructuredToolResult::WriteFile {
                path: path.display().to_string(),
                bytes_written,
                status: WriteFileStatus::Completed,
            }),
        })
    }
}

fn resolve_workspace_path(workspace_root: &Path, value: Option<&str>) -> Result<PathBuf> {
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let Some(value) = value else {
        return Ok(root);
    };

    let input = Path::new(value);
    if input.is_absolute() {
        bail!("absolute paths are not allowed; use workspace-relative paths");
    }

    let mut candidate = root.clone();
    for component in input.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => candidate.push(segment),
            Component::ParentDir => {
                if !candidate.pop() || !candidate.starts_with(&root) {
                    bail!("path escapes the workspace root");
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                bail!("unsupported path component")
            }
        }
    }

    if !candidate.starts_with(&root) {
        bail!("path escapes the workspace root");
    }

    Ok(candidate)
}
