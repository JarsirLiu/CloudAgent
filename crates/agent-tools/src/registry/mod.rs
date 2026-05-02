use crate::impls::{
    code_editing::{ApplyPatchTool, WriteFileToolV2},
    command_execution::ShellCommandToolV2,
    repository_exploration::{
        FindFilesArgs, FindFilesTool, ReadFileToolV2, ReadFilesArgs, ReadFilesTool, SearchTextArgs,
        SearchTextTool, run_search_text,
    },
    workspace_file_ops::{GetMetadataTool, ReadDirectoryToolV2},
};
use crate::selection::{TaskKind, ToolMode, ToolSelector};
use crate::spec::ToolDescriptor;
use agent_core::{
    ToolCall, ToolExecutionContext, ToolExecutor, ToolOutputDelta, ToolOutputStream, ToolResult,
    ToolSpec,
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

#[async_trait]
trait LocalTool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput>;
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
    descriptors: Vec<ToolDescriptor>,
    selector: ToolSelector,
}

impl ToolRegistry {
    pub fn new(max_read_chars: usize) -> Self {
        let descriptors = vec![
            SearchTextTool::descriptor(),
            FindFilesTool::descriptor(),
            ReadFileToolV2::descriptor(max_read_chars),
            ReadFilesTool::descriptor(max_read_chars),
            ApplyPatchTool::descriptor(),
            WriteFileToolV2::descriptor(),
            ShellCommandToolV2::descriptor(),
            GetMetadataTool::descriptor(),
            ReadDirectoryToolV2::descriptor(),
        ];

        let mut tools: BTreeMap<String, Arc<dyn LocalTool>> = BTreeMap::new();
        register(&mut tools, ShellCommandTool);
        register(&mut tools, SearchTextLocalTool);
        register(&mut tools, FindFilesLocalTool);
        register(&mut tools, ReadFilesLocalTool { max_read_chars });
        register(&mut tools, GetMetadataLocalTool);
        register(&mut tools, ReadDirectoryTool);
        register(&mut tools, ReadFileTool { max_read_chars });
        register(&mut tools, WriteFileTool);

        Self {
            tools,
            descriptors,
            selector: ToolSelector::new(),
        }
    }

    pub fn all_descriptors(&self) -> &[ToolDescriptor] {
        &self.descriptors
    }

    pub fn specs_for_mode(&self, mode: ToolMode, task_kind: TaskKind) -> Vec<ToolSpec> {
        self.selector
            .select(&mode, &task_kind, &self.descriptors)
            .into_iter()
            .map(|descriptor| descriptor.spec.clone())
            .collect()
    }
}

#[async_trait]
impl ToolExecutor for ToolRegistry {
    fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    fn specs_for_context(&self, mode: &str, task_kind: &str) -> Vec<ToolSpec> {
        let parsed_mode = match mode {
            "explore" => ToolMode::Explore,
            "edit" => ToolMode::Edit,
            "verify" => ToolMode::Verify,
            _ => ToolMode::Full,
        };
        let parsed_task = match task_kind {
            "repository_analysis" => TaskKind::RepositoryAnalysis,
            "code_edit" => TaskKind::CodeEdit,
            "verification" => TaskKind::Verification,
            "workspace_file_operation" => TaskKind::WorkspaceFileOperation,
            _ => TaskKind::General,
        };
        self.specs_for_mode(parsed_mode, parsed_task)
    }

    async fn execute(&self, call: ToolCall, ctx: &ToolExecutionContext) -> Result<ToolResult> {
        let call_name = call.name.clone();
        let call_args = call.arguments.clone();
        let Some(tool) = self.tools.get(&call.name) else {
            bail!("tool `{}` is not registered", call.name);
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
struct SearchTextLocalTool;
struct FindFilesLocalTool;
struct ReadFilesLocalTool {
    max_read_chars: usize,
}
struct GetMetadataLocalTool;
struct ReadDirectoryTool;
struct ReadFileTool {
    max_read_chars: usize,
}
struct WriteFileTool;

#[derive(Deserialize)]
struct ShellCommandArgs {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct ReadDirectoryArgs {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct ReadFileArgs {
    path: String,
    #[serde(default)]
    max_chars: Option<usize>,
}

#[derive(Deserialize)]
struct GetMetadataArgs {
    path: String,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
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
        ShellCommandToolV2::descriptor().spec
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
            summary: shell_summary(
                &args.command,
                &current_directory,
                exit_code,
                status.success(),
                &stdout,
            ),
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

fn shell_summary(
    _command: &str,
    _current_directory: &str,
    exit_code: i32,
    _success: bool,
    _stdout: &str,
) -> String {
    format!("shell command finished with exit code {exit_code}")
}

#[async_trait]
impl LocalTool for SearchTextLocalTool {
    fn spec(&self) -> ToolSpec {
        SearchTextTool::descriptor().spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: SearchTextArgs = serde_json::from_value(arguments)?;
        let output = run_search_text(&ctx.workspace_root, args).await?;
        let lines = output
            .results
            .iter()
            .map(|m| format!("{}:{}: {}", m.path, m.line, m.preview))
            .collect::<Vec<_>>()
            .join("\n");
        let content = if lines.is_empty() {
            "No matches found".to_string()
        } else {
            format!("Found {} matches in {} files.\n{}", output.match_count, output.file_count, lines)
        };
        Ok(ToolInvocationOutput {
            content,
            summary: format!("found {} matches across {} files", output.match_count, output.file_count),
            structured: None,
        })
    }
}

#[async_trait]
impl LocalTool for FindFilesLocalTool {
    fn spec(&self) -> ToolSpec {
        FindFilesTool::descriptor().spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: FindFilesArgs = serde_json::from_value(arguments)?;
        let pattern = args.pattern.trim().to_lowercase();
        if pattern.is_empty() {
            bail!("`pattern` must not be empty");
        }
        let max_results = args.max_results.unwrap_or(200).clamp(1, 2_000);
        let root = resolve_workspace_path(&ctx.workspace_root, args.path_scope.as_deref())?;
        let mut stack = vec![root];
        let mut matches = Vec::new();
        let ignored = [
            ".git", ".hg", ".svn", "node_modules", "dist", "build", "target", "target-verify",
            ".next", ".nuxt", ".turbo", ".cache", "coverage", ".venv", "venv", "__pycache__",
        ];

        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let metadata = match entry.metadata().await {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                if metadata.is_dir() {
                    if ignored.contains(&name.as_str()) || (name.starts_with('.') && name != ".cargo")
                    {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                if metadata.is_file() && name.to_lowercase().contains(&pattern) {
                    let rel = path
                        .strip_prefix(&ctx.workspace_root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    matches.push(rel);
                    if matches.len() >= max_results {
                        break;
                    }
                }
            }
            if matches.len() >= max_results {
                break;
            }
        }

        matches.sort();
        let content = if matches.is_empty() {
            "No files found".to_string()
        } else {
            matches.join("\n")
        };
        Ok(ToolInvocationOutput {
            summary: format!("found {} files", matches.len()),
            content,
            structured: None,
        })
    }
}

#[async_trait]
impl LocalTool for ReadFilesLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFilesTool::descriptor(self.max_read_chars).spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadFilesArgs = serde_json::from_value(arguments)?;
        if args.paths.is_empty() {
            bail!("`paths` must not be empty");
        }
        let max_lines = args.max_lines_per_file.unwrap_or(300).clamp(1, 2_000);
        let mut blocks = Vec::new();

        for path in args.paths {
            let resolved = resolve_workspace_path(&ctx.workspace_root, Some(path.as_str()))?;
            let text = fs::read_to_string(&resolved).await?;
            let mut lines = Vec::new();
            for (idx, line) in text.lines().enumerate() {
                if idx >= max_lines {
                    lines.push("[truncated]".to_string());
                    break;
                }
                lines.push(line.to_string());
            }
            let rel = resolved
                .strip_prefix(&ctx.workspace_root)
                .unwrap_or(&resolved)
                .to_string_lossy()
                .replace('\\', "/");
            blocks.push(format!("== {} ==\n{}", rel, lines.join("\n")));
        }

        Ok(ToolInvocationOutput {
            summary: format!("read {} files", blocks.len()),
            content: blocks.join("\n\n"),
            structured: None,
        })
    }
}

#[async_trait]
impl LocalTool for GetMetadataLocalTool {
    fn spec(&self) -> ToolSpec {
        GetMetadataTool::descriptor().spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: GetMetadataArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let metadata = fs::metadata(&path).await?;
        let value = json!({
            "path": path.display().to_string(),
            "exists": true,
            "is_file": metadata.is_file(),
            "is_dir": metadata.is_dir(),
            "size": metadata.len(),
            "readonly": metadata.permissions().readonly()
        });
        Ok(ToolInvocationOutput {
            summary: format!("metadata for {}", path.display()),
            content: serde_json::to_string_pretty(&value)?,
            structured: None,
        })
    }
}

#[async_trait]
impl LocalTool for ReadDirectoryTool {
    fn spec(&self) -> ToolSpec {
        ReadDirectoryToolV2::descriptor().spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadDirectoryArgs = serde_json::from_value(arguments)?;
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
        items.sort_by(|l, r| l["name"].as_str().unwrap_or_default().cmp(r["name"].as_str().unwrap_or_default()));
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

#[async_trait]
impl LocalTool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ReadFileToolV2::descriptor(self.max_read_chars).spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let text = fs::read_to_string(&path).await?;
        let max_chars = args.max_chars.unwrap_or(self.max_read_chars).max(128);
        let content = if text.chars().count() > max_chars {
            format!("{}\n\n[truncated]", text.chars().take(max_chars).collect::<String>())
        } else {
            text
        };
        let char_count = content.chars().count();
        let truncated = content.ends_with("\n\n[truncated]");
        Ok(ToolInvocationOutput {
            summary: format!("read {}", path.display()),
            content,
            structured: Some(StructuredToolResult::ReadFile {
                path: path.display().to_string(),
                truncated,
                char_count,
            }),
        })
    }
}

#[async_trait]
impl LocalTool for WriteFileTool {
    fn spec(&self) -> ToolSpec {
        WriteFileToolV2::descriptor().spec
    }

    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
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
    let root = workspace_root.canonicalize().unwrap_or_else(|_| workspace_root.to_path_buf());
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
            Component::Prefix(_) | Component::RootDir => bail!("unsupported path component"),
        }
    }
    if !candidate.starts_with(&root) {
        bail!("path escapes the workspace root");
    }
    Ok(candidate)
}
