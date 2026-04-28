use agent_core::{
    ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, ToolSpec, context::ToolExecutionContext as CtxAlias,
};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::fs;
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
}

#[derive(Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn LocalTool>>,
}

impl ToolRegistry {
    pub fn new(max_read_chars: usize) -> Self {
        let mut tools: HashMap<String, Arc<dyn LocalTool>> = HashMap::new();
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
        let Some(tool) = self.tools.get(&call.name) else {
            return Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: "Tool not found".to_string(),
                summary: "tool lookup failed".to_string(),
                is_error: true,
            });
        };

        match tool.invoke(call.arguments, ctx).await {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: output.content,
                summary: output.summary,
                is_error: false,
            }),
            Err(err) => Ok(ToolResult {
                tool_call_id: call.id,
                name: call.name,
                content: format!("Tool execution failed: {err:#}"),
                summary: "tool execution failed".to_string(),
                is_error: true,
            }),
        }
    }
}

fn register<T>(tools: &mut HashMap<String, Arc<dyn LocalTool>>, tool: T)
where
    T: LocalTool + 'static,
{
    tools.insert(tool.spec().name.clone(), Arc::new(tool));
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
        }
    }

    async fn invoke(&self, arguments: Value, ctx: &CtxAlias) -> Result<ToolInvocationOutput> {
        let args: ShellCommandArgs = serde_json::from_value(arguments)?;
        let workdir = resolve_workspace_path(&ctx.workspace_root, args.workdir.as_deref())?;
        let timeout_ms = args.timeout_ms.unwrap_or(ctx.default_shell_timeout_ms).max(1_000);

        let mut command = if cfg!(windows) {
            let mut cmd = Command::new("powershell");
            cmd.arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(&args.command);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-lc").arg(&args.command);
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
        let stdout_task = tokio::spawn(async move {
            let mut reader = stdout;
            let mut buffer = Vec::new();
            reader.read_to_end(&mut buffer).await?;
            Result::<Vec<u8>>::Ok(buffer)
        });
        let stderr_task = tokio::spawn(async move {
            let mut reader = stderr;
            let mut buffer = Vec::new();
            reader.read_to_end(&mut buffer).await?;
            Result::<Vec<u8>>::Ok(buffer)
        });

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

        let stdout = String::from_utf8_lossy(&stdout_task.await??).to_string();
        let stderr = String::from_utf8_lossy(&stderr_task.await??).to_string();

        let exit_code = status.code().unwrap_or(-1);
        let content = serde_json::to_string_pretty(&json!({
            "command": args.command,
            "workdir": workdir.display().to_string(),
            "exit_code": exit_code,
            "success": status.success(),
            "stdout": stdout,
            "stderr": stderr,
        }))?;

        Ok(ToolInvocationOutput {
            content,
            summary: format!("shell command finished with exit code {exit_code}"),
        })
    }
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
            description: "Read a text file from the workspace. Large outputs are truncated.".to_string(),
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

        Ok(ToolInvocationOutput {
            content,
            summary: format!("read {}", path.display()),
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
        }
    }

    async fn invoke(&self, arguments: Value, ctx: &CtxAlias) -> Result<ToolInvocationOutput> {
        let args: WriteFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let Some(parent) = path.parent() else {
            bail!("cannot determine parent directory for {}", path.display());
        };
        fs::create_dir_all(parent).await?;
        fs::write(&path, args.content).await?;

        Ok(ToolInvocationOutput {
            content: format!("Wrote {}", path.display()),
            summary: format!("wrote {}", path.display()),
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
