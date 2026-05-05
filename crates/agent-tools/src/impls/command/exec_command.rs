use crate::impls::repo::DEFAULT_IGNORED_DIRS;
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, decode_utf8_chunk, resolve_write_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{
    ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolOutputStream, ToolSpec,
};
use agent_protocol::{CommandExecutionStatus, StructuredToolResult};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep, timeout};

pub struct ExecCommandTool;

const MAX_CAPTURE_CHARS_PER_STREAM: usize = 24_000;
const MAX_LIVE_OUTPUT_CHARS_PER_STREAM: usize = 12_000;
const OUTPUT_TRUNCATION_NOTICE: &str = "\n[output truncated; narrow the command or use `search_workspace` and follow with `read_file` for repository discovery]\n";

impl ExecCommandTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::CommandExecution,
            ToolRisk::High,
            ToolPermissionTier::ReadOnly,
            vec!["edit", "verify"],
            ToolUsageGuidance {
                selection_priority: 15,
                preferred_for: vec![
                    "build, test, git, and runtime verification",
                    "interactive command sessions",
                ],
                avoid_for: vec!["workspace file edits", "repository search when structured tools are available"],
                follow_up_hint: Some("prefer `workdir` over inline `cd`; on Windows use PowerShell syntax"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "exec_command".to_string(),
                identity: ToolIdentity::built_in("exec_command"),
                description: "Run local commands for build, test, git, and runtime verification. Reuse `session_id` for interactive or long-running command sessions.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "workdir": { "type": "string" },
                        "timeout_ms": { "type": "integer", "minimum": 1000 },
                        "start_new_session": { "type": "boolean" },
                        "session_id": { "type": "string" },
                        "stdin": { "type": "string" },
                        "close_stdin": { "type": "boolean" },
                        "wait_for_exit": { "type": "boolean" }
                    }
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: true,
                item_kind: agent_protocol::TurnItemKind::CommandExecution,
                delta_kind: agent_protocol::TurnItemDeltaKind::CommandExecutionOutput,
                approval_reason: Some("Local command execution can inspect or modify the workspace.".to_string()),
            },
        )
    }
}

#[derive(Default)]
struct ExecSessionStore {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<String, Arc<ExecSession>>>,
}

impl ExecSessionStore {
    fn new() -> Self {
        Self::default()
    }

    fn allocate_id(&self, conversation_id: &str) -> String {
        let next = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("exec:{}:{next}", conversation_id)
    }

    async fn insert(&self, id: String, session: Arc<ExecSession>) {
        self.sessions.lock().await.insert(id, session);
    }

    async fn get(&self, id: &str) -> Option<Arc<ExecSession>> {
        self.sessions.lock().await.get(id).cloned()
    }

    async fn remove(&self, id: &str) -> Option<Arc<ExecSession>> {
        self.sessions.lock().await.remove(id)
    }
}

struct ExecSession {
    command: String,
    current_directory: String,
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    stdout_cursor: Mutex<usize>,
    stderr_cursor: Mutex<usize>,
}

#[derive(Clone, Debug)]
struct CapturedOutput {
    text: String,
}

impl CapturedOutput {
    fn new(text: String) -> Self {
        Self { text }
    }
}

impl ExecSession {
    async fn write_stdin(&self, text: &str) -> Result<()> {
        let mut guard = self.stdin.lock().await;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| anyhow!("stdin is no longer available for this session"))?;
        stdin.write_all(text.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn close_stdin(&self) {
        let _ = self.stdin.lock().await.take();
    }

    async fn take_new_stdout(&self) -> String {
        take_new_buffer(&self.stdout, &self.stdout_cursor).await
    }

    async fn take_new_stderr(&self) -> String {
        take_new_buffer(&self.stderr, &self.stderr_cursor).await
    }
}

#[derive(Deserialize)]
struct ExecCommandArgs {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    start_new_session: Option<bool>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    stdin: Option<String>,
    #[serde(default)]
    close_stdin: Option<bool>,
    #[serde(default)]
    wait_for_exit: Option<bool>,
}

pub(crate) struct ExecCommandLocalTool {
    sessions: Arc<ExecSessionStore>,
}

impl ExecCommandLocalTool {
    pub(crate) fn new() -> Self {
        Self {
            sessions: Arc::new(ExecSessionStore::new()),
        }
    }
}

#[async_trait]
impl LocalTool for ExecCommandLocalTool {
    fn spec(&self) -> ToolSpec {
        ExecCommandTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ExecCommandArgs = invocation.payload.parse_arguments()?;
        let timeout_ms = args
            .timeout_ms
            .unwrap_or(ctx.default_shell_timeout_ms)
            .max(1_000);

        if let Some(session_id) = args.session_id.clone() {
            return self
                .resume_session(&session_id, args, timeout_ms, ctx)
                .await;
        }

        let command = args
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("`command` is required"))?;
        let workdir = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            args.workdir.as_deref(),
        )?;
        if looks_like_apply_patch_command(command) {
            return Ok(reject_patch_via_exec_command(command, &workdir));
        }
        if args.start_new_session.unwrap_or(false) {
            return self
                .start_session(
                    command,
                    workdir,
                    timeout_ms,
                    args.wait_for_exit.unwrap_or(false),
                    ctx,
                )
                .await;
        }
        if args.stdin.is_some() || args.close_stdin.unwrap_or(false) {
            bail!("`stdin` and `close_stdin` require a `session_id`");
        }

        run_one_shot_command(command, workdir, timeout_ms, ctx).await
    }
}

impl ExecCommandLocalTool {
    async fn start_session(
        &self,
        command: &str,
        workdir: std::path::PathBuf,
        timeout_ms: u64,
        wait_for_exit: bool,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let started_at = Instant::now();
        let rendered_command =
            translate_search_command(command).unwrap_or_else(|| command.to_string());
        let mut child = build_command_process(&rendered_command, &workdir);
        let mut child = child.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to capture command stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to capture command stderr"))?;

        let stdout_buffer = Arc::new(Mutex::new(String::new()));
        let stderr_buffer = Arc::new(Mutex::new(String::new()));
        let stdout_tx = ctx.output_tx.clone();
        let stderr_tx = ctx.output_tx.clone();
        tokio::spawn(pump_exec_reader(
            stdout,
            ToolOutputStream::Stdout,
            stdout_buffer.clone(),
            stdout_tx,
            MAX_CAPTURE_CHARS_PER_STREAM,
            MAX_LIVE_OUTPUT_CHARS_PER_STREAM,
        ));
        tokio::spawn(pump_exec_reader(
            stderr,
            ToolOutputStream::Stderr,
            stderr_buffer.clone(),
            stderr_tx,
            MAX_CAPTURE_CHARS_PER_STREAM,
            MAX_LIVE_OUTPUT_CHARS_PER_STREAM,
        ));

        let session = Arc::new(ExecSession {
            command: command.to_string(),
            current_directory: workdir.display().to_string(),
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: stdout_buffer,
            stderr: stderr_buffer,
            stdout_cursor: Mutex::new(0),
            stderr_cursor: Mutex::new(0),
        });
        let session_id = self.sessions.allocate_id(&ctx.conversation_id);
        self.sessions
            .insert(session_id.clone(), session.clone())
            .await;

        sleep(Duration::from_millis(50)).await;
        let output = self
            .build_session_result(
                &session_id,
                session,
                timeout_ms,
                wait_for_exit,
                started_at,
                ctx,
            )
            .await?;
        if !matches!(
            output.structured,
            Some(StructuredToolResult::CommandExecution {
                status: CommandExecutionStatus::InProgress,
                ..
            })
        ) {
            let _ = self.sessions.remove(&session_id).await;
        }
        Ok(output)
    }

    async fn resume_session(
        &self,
        session_id: &str,
        args: ExecCommandArgs,
        timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let Some(session) = self.sessions.get(session_id).await else {
            bail!("exec session `{session_id}` was not found");
        };
        if let Some(stdin) = args.stdin.as_deref() {
            session.write_stdin(stdin).await?;
        }
        if args.close_stdin.unwrap_or(false) {
            session.close_stdin().await;
        }
        sleep(Duration::from_millis(50)).await;
        let output = self
            .build_session_result(
                session_id,
                session.clone(),
                timeout_ms,
                args.wait_for_exit.unwrap_or(false),
                Instant::now(),
                ctx,
            )
            .await?;
        if !matches!(
            output.structured,
            Some(StructuredToolResult::CommandExecution {
                status: CommandExecutionStatus::InProgress,
                ..
            })
        ) {
            let _ = self.sessions.remove(session_id).await;
        }
        Ok(output)
    }

    async fn build_session_result(
        &self,
        session_id: &str,
        session: Arc<ExecSession>,
        timeout_ms: u64,
        wait_for_exit: bool,
        started_at: Instant,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let (status, exit_code, success) = wait_for_session(
            &session.child,
            timeout_ms,
            wait_for_exit,
            &ctx.cancellation_token,
        )
        .await?;
        let stdout = session.take_new_stdout().await;
        let stderr = session.take_new_stderr().await;
        let duration_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let content = format_exec_result_content(
            summarize_command(&session.command),
            &session.command,
            &session.current_directory,
            Some(session_id),
            status.clone(),
            exit_code,
            success,
            &stdout,
            &stderr,
        );
        Ok(ToolInvocationOutput {
            content: content.clone(),
            structured: Some(StructuredToolResult::CommandExecution {
                command: session.command.clone(),
                current_directory: session.current_directory.clone(),
                session_id: Some(session_id.to_string()),
                status,
                exit_code,
                success,
                stdout: Some(stdout),
                stderr: Some(stderr),
                aggregated_output: Some(content),
                duration_ms: Some(duration_ms),
            }),
        })
    }
}

async fn run_one_shot_command(
    command: &str,
    workdir: std::path::PathBuf,
    timeout_ms: u64,
    ctx: &ToolExecutionContext,
) -> Result<ToolInvocationOutput> {
    let started_at = Instant::now();
    let rendered_command = translate_search_command(command).unwrap_or_else(|| command.to_string());
    let mut child = build_command_process(&rendered_command, &workdir).spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture command stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to capture command stderr"))?;
    let stdout_task = tokio::spawn(read_streaming_pipe_limited(
        stdout,
        ToolOutputStream::Stdout,
        ctx.output_tx.clone(),
        MAX_CAPTURE_CHARS_PER_STREAM,
        MAX_LIVE_OUTPUT_CHARS_PER_STREAM,
    ));
    let stderr_task = tokio::spawn(read_streaming_pipe_limited(
        stderr,
        ToolOutputStream::Stderr,
        ctx.output_tx.clone(),
        MAX_CAPTURE_CHARS_PER_STREAM,
        MAX_LIVE_OUTPUT_CHARS_PER_STREAM,
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
    let stdout = stdout_task.await??.text.trim().to_string();
    let stderr = stderr_task.await??.text.trim().to_string();
    let duration_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let exit_code = status.code().unwrap_or(-1);
    let current_directory = workdir.display().to_string();
    let content = format_exec_result_content(
        summarize_command(command),
        command,
        &current_directory,
        None,
        if status.success() {
            CommandExecutionStatus::Completed
        } else {
            CommandExecutionStatus::Failed
        },
        Some(exit_code),
        Some(status.success()),
        &stdout,
        &stderr,
    );
    Ok(ToolInvocationOutput {
        content: content.clone(),
        structured: Some(StructuredToolResult::CommandExecution {
            command: command.to_string(),
            current_directory,
            session_id: None,
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

fn build_command_process(command_text: &str, workdir: &std::path::Path) -> Command {
    let mut command = if cfg!(windows) {
        let mut cmd = Command::new(preferred_windows_shell());
        cmd.arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(windows_utf8_command(command_text));
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-lc").arg(command_text);
        cmd
    };
    command
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.kill_on_drop(true);
    command
}

#[allow(clippy::too_many_arguments)]
fn format_exec_result_content(
    kind: &str,
    command: &str,
    current_directory: &str,
    _session_id: Option<&str>,
    status: CommandExecutionStatus,
    exit_code: Option<i32>,
    success: Option<bool>,
    stdout: &str,
    stderr: &str,
) -> String {
    let mut lines = vec![
        format!("kind: {kind}"),
        format!("command: {command}"),
        format!("current_directory: {current_directory}"),
    ];
    lines.push(format!("status: {}", render_command_status(&status)));
    if let Some(exit_code) = exit_code {
        lines.push(format!("exit_code: {exit_code}"));
    }
    if let Some(success) = success {
        lines.push(format!("success: {success}"));
    }
    lines.push(String::new());
    lines.push("stdout:".to_string());
    lines.push(if stdout.is_empty() {
        "(empty)".to_string()
    } else {
        stdout.to_string()
    });
    lines.push(String::new());
    lines.push("stderr:".to_string());
    lines.push(if stderr.is_empty() {
        "(empty)".to_string()
    } else {
        stderr.to_string()
    });
    lines.join("\n")
}

fn render_command_status(status: &CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::InProgress => "in_progress",
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::Declined => "declined",
    }
}

fn looks_like_apply_patch_command(command: &str) -> bool {
    let normalized = command.trim().to_ascii_lowercase();
    normalized.starts_with("apply_patch ")
        || normalized == "apply_patch"
        || normalized.contains("\napply_patch ")
        || normalized.contains("&& apply_patch ")
        || normalized.contains("; apply_patch ")
}

fn reject_patch_via_exec_command(command: &str, workdir: &std::path::Path) -> ToolInvocationOutput {
    let current_directory = workdir.display().to_string();
    let message = "Use the dedicated file editing tool instead of exec_command for workspace file edits. Prefer `edit_file` for structured replacements in known files.".to_string();
    let content = format_exec_result_content(
        "edit",
        command,
        &current_directory,
        None,
        CommandExecutionStatus::Failed,
        None,
        Some(false),
        "",
        &message,
    );
    ToolInvocationOutput {
        content: content.clone(),
        structured: Some(StructuredToolResult::CommandExecution {
            command: command.to_string(),
            current_directory,
            session_id: None,
            status: CommandExecutionStatus::Failed,
            exit_code: None,
            success: Some(false),
            stdout: Some(String::new()),
            stderr: Some(message),
            aggregated_output: Some(content),
            duration_ms: Some(0),
        }),
    }
}

async fn wait_for_session(
    child: &Mutex<Child>,
    timeout_ms: u64,
    wait_for_exit: bool,
    cancellation_token: &tokio_util::sync::CancellationToken,
) -> Result<(CommandExecutionStatus, Option<i32>, Option<bool>)> {
    let mut child = child.lock().await;
    let exited = if wait_for_exit {
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                bail!("command aborted by user");
            }
            waited = timeout(Duration::from_millis(timeout_ms), child.wait()) => {
                match waited {
                    Ok(result) => Some(result?),
                    Err(_) => None,
                }
            }
        }
    } else {
        child.try_wait()?
    };

    match exited {
        Some(status) => Ok((
            if status.success() {
                CommandExecutionStatus::Completed
            } else {
                CommandExecutionStatus::Failed
            },
            Some(status.code().unwrap_or(-1)),
            Some(status.success()),
        )),
        None => Ok((CommandExecutionStatus::InProgress, None, None)),
    }
}

async fn pump_exec_reader<R>(
    mut reader: R,
    stream: ToolOutputStream,
    buffer: Arc<Mutex<String>>,
    output_tx: Option<tokio::sync::mpsc::UnboundedSender<agent_core::ToolOutputDelta>>,
    capture_limit_chars: usize,
    live_limit_chars: usize,
) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut raw = [0_u8; 8192];
    let mut pending_utf8 = Vec::new();
    let mut capture_truncated = false;
    let mut live_truncated = false;
    let mut live_chars_sent = 0usize;
    loop {
        let read = reader.read(&mut raw).await?;
        if read == 0 {
            break;
        }
        pending_utf8.extend_from_slice(&raw[..read]);
        let chunk = decode_utf8_chunk(&mut pending_utf8, false);
        if !chunk.is_empty() {
            append_capped_chunk(
                &mut *buffer.lock().await,
                &chunk,
                capture_limit_chars,
                &mut capture_truncated,
            );
            if let Some(output_tx) = &output_tx {
                if let Some(delta) = take_live_chunk(
                    &chunk,
                    live_limit_chars,
                    &mut live_chars_sent,
                    &mut live_truncated,
                ) {
                    let _ = output_tx.send(agent_core::ToolOutputDelta {
                        stream: stream.clone(),
                        chunk: delta,
                    });
                }
            }
        }
    }
    let tail = decode_utf8_chunk(&mut pending_utf8, true);
    if !tail.is_empty() {
        append_capped_chunk(
            &mut *buffer.lock().await,
            &tail,
            capture_limit_chars,
            &mut capture_truncated,
        );
        if let Some(output_tx) = &output_tx {
            if let Some(delta) = take_live_chunk(
                &tail,
                live_limit_chars,
                &mut live_chars_sent,
                &mut live_truncated,
            ) {
                let _ = output_tx.send(agent_core::ToolOutputDelta {
                    stream,
                    chunk: delta,
                });
            }
        }
    }
    Ok(())
}

async fn read_streaming_pipe_limited<R>(
    mut reader: R,
    stream: ToolOutputStream,
    output_tx: Option<tokio::sync::mpsc::UnboundedSender<agent_core::ToolOutputDelta>>,
    capture_limit_chars: usize,
    live_limit_chars: usize,
) -> Result<CapturedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut raw = [0_u8; 8192];
    let mut pending_utf8 = Vec::new();
    let mut captured = String::new();
    let mut capture_truncated = false;
    let mut live_truncated = false;
    let mut live_chars_sent = 0usize;

    loop {
        let read = reader.read(&mut raw).await?;
        if read == 0 {
            break;
        }
        pending_utf8.extend_from_slice(&raw[..read]);
        let chunk = decode_utf8_chunk(&mut pending_utf8, false);
        if !chunk.is_empty() {
            append_capped_chunk(
                &mut captured,
                &chunk,
                capture_limit_chars,
                &mut capture_truncated,
            );
            if let Some(output_tx) = &output_tx {
                if let Some(delta) = take_live_chunk(
                    &chunk,
                    live_limit_chars,
                    &mut live_chars_sent,
                    &mut live_truncated,
                ) {
                    let _ = output_tx.send(agent_core::ToolOutputDelta {
                        stream: stream.clone(),
                        chunk: delta,
                    });
                }
            }
        }
    }

    let tail = decode_utf8_chunk(&mut pending_utf8, true);
    if !tail.is_empty() {
        append_capped_chunk(
            &mut captured,
            &tail,
            capture_limit_chars,
            &mut capture_truncated,
        );
        if let Some(output_tx) = &output_tx {
            if let Some(delta) = take_live_chunk(
                &tail,
                live_limit_chars,
                &mut live_chars_sent,
                &mut live_truncated,
            ) {
                let _ = output_tx.send(agent_core::ToolOutputDelta {
                    stream,
                    chunk: delta,
                });
            }
        }
    }

    Ok(CapturedOutput::new(captured))
}

fn append_capped_chunk(buffer: &mut String, chunk: &str, limit_chars: usize, truncated: &mut bool) {
    if *truncated {
        return;
    }
    let current_chars = buffer.chars().count();
    if current_chars >= limit_chars {
        buffer.push_str(OUTPUT_TRUNCATION_NOTICE);
        *truncated = true;
        return;
    }
    let remaining = limit_chars.saturating_sub(current_chars);
    let chunk_chars = chunk.chars().count();
    if chunk_chars <= remaining {
        buffer.push_str(chunk);
        return;
    }
    buffer.push_str(&chunk.chars().take(remaining).collect::<String>());
    buffer.push_str(OUTPUT_TRUNCATION_NOTICE);
    *truncated = true;
}

fn take_live_chunk(
    chunk: &str,
    limit_chars: usize,
    live_chars_sent: &mut usize,
    truncated: &mut bool,
) -> Option<String> {
    if *truncated {
        return None;
    }
    if *live_chars_sent >= limit_chars {
        *truncated = true;
        return Some(OUTPUT_TRUNCATION_NOTICE.to_string());
    }
    let remaining = limit_chars.saturating_sub(*live_chars_sent);
    let chunk_chars = chunk.chars().count();
    if chunk_chars <= remaining {
        *live_chars_sent += chunk_chars;
        return Some(chunk.to_string());
    }
    let mut rendered = chunk.chars().take(remaining).collect::<String>();
    rendered.push_str(OUTPUT_TRUNCATION_NOTICE);
    *live_chars_sent = limit_chars;
    *truncated = true;
    Some(rendered)
}

async fn take_new_buffer(buffer: &Arc<Mutex<String>>, cursor: &Mutex<usize>) -> String {
    let text = buffer.lock().await.clone();
    let mut cursor = cursor.lock().await;
    let start = (*cursor).min(text.len());
    let out = text[start..].to_string();
    *cursor = text.len();
    out.trim().to_string()
}

fn summarize_command(command: &str) -> &'static str {
    let normalized = command.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return "unknown";
    }
    if contains_write_operator(&normalized) || contains_network_indicator(&normalized) {
        return "action";
    }

    let Some(program) = normalized.split_whitespace().next() else {
        return "unknown";
    };

    match program {
        "rg" | "grep" | "findstr" | "select-string" => "search",
        "git" if normalized.starts_with("git ls-files") => "list files",
        "git" if normalized.starts_with("git grep") => "search",
        "git"
            if normalized.starts_with("git log")
                || normalized.starts_with("git status")
                || normalized.starts_with("git diff")
                || normalized.starts_with("git show")
                || normalized.starts_with("git branch")
                || normalized.starts_with("git rev-parse")
                || normalized.starts_with("git cat-file") =>
        {
            "inspect"
        }
        "pwd" | "ls" | "dir" | "cat" | "type" | "head" | "tail" | "find" | "tree" => "inspect",
        "fd" => "find files",
        "get-childitem" | "get-content" => "inspect",
        "measure-object" | "where-object" | "sort-object" | "select-object" => "inspect",
        _ => "action",
    }
}

fn translate_search_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let normalized = trimmed.to_ascii_lowercase();
    if !normalized.starts_with("rg") {
        return None;
    }

    if command_exists("rg") || command_exists("rg.exe") {
        return None;
    }

    translate_rg_command(trimmed)
}

fn translate_rg_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let normalized = trimmed.to_ascii_lowercase();
    if !normalized.starts_with("rg") {
        return None;
    }

    if normalized.starts_with("rg --files") || normalized.starts_with("rg.exe --files") {
        let path_scope = extract_rg_path_scope(trimmed).unwrap_or_else(|| ".".to_string());
        return Some(if cfg!(windows) {
            format!(
                "{} | Select-Object -ExpandProperty FullName",
                windows_bounded_file_listing_command(&path_scope)
            )
        } else {
            unix_bounded_file_listing_command(&path_scope)
        });
    }

    let (pattern, path_scope, case_sensitive) = extract_rg_search_args(trimmed)?;
    Some(if cfg!(windows) {
        let mut command = format!(
            "{} | Select-String -Pattern {}",
            windows_bounded_file_listing_command(&path_scope),
            powershell_quote(&pattern)
        );
        if !case_sensitive {
            command.push_str(" -CaseSensitive:$false");
        }
        command
    } else {
        let mut command = format!(
            "{} | xargs -0 grep -n -I",
            unix_bounded_file_listing_command(&path_scope)
        );
        if !case_sensitive {
            command.push_str(" -i");
        }
        command.push_str(" -e ");
        command.push_str(&shell_quote(&pattern));
        command
    })
}

fn windows_bounded_file_listing_command(path_scope: &str) -> String {
    let mut command = format!(
        "Get-ChildItem -Recurse -File {}",
        powershell_quote(path_scope)
    );
    for ignored in DEFAULT_IGNORED_DIRS {
        command.push_str(&format!(
            " | Where-Object {{ $_.FullName -notmatch '[\\\\/]{ignored}([\\\\/]|$)' }}"
        ));
    }
    command
}

fn unix_bounded_file_listing_command(path_scope: &str) -> String {
    let mut command = format!("find {}", shell_quote(path_scope));
    for ignored in DEFAULT_IGNORED_DIRS {
        command.push_str(&format!(
            " -path {} -prune -o",
            shell_quote(&format!("*/{ignored}"))
        ));
    }
    command.push_str(" -type f -print0");
    command
}

fn extract_rg_search_args(command: &str) -> Option<(String, String, bool)> {
    let mut rest = command.trim();
    let program = take_shell_token(&mut rest)?.to_ascii_lowercase();
    if program != "rg" && program != "rg.exe" {
        return None;
    }

    let mut case_sensitive = true;
    let mut pattern = None;
    let mut path_scope = ".".to_string();
    while !rest.trim().is_empty() {
        let token = take_shell_token(&mut rest)?;
        match token.as_str() {
            "-i" | "--ignore-case" => case_sensitive = false,
            "-n" | "-H" | "--with-filename" | "--no-heading" | "--line-number" | "-S" => {}
            "--files" => return None,
            t if t.starts_with('-') => {}
            t if pattern.is_none() => pattern = Some(t.to_string()),
            t => path_scope = t.to_string(),
        }
    }

    Some((pattern?, path_scope, case_sensitive))
}

fn extract_rg_path_scope(command: &str) -> Option<String> {
    let mut rest = command.trim();
    let program = take_shell_token(&mut rest)?.to_ascii_lowercase();
    if program != "rg" && program != "rg.exe" {
        return None;
    }

    let mut path_scope = ".".to_string();
    while !rest.trim().is_empty() {
        let token = take_shell_token(&mut rest)?;
        if token == "--files" {
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        path_scope = token;
        break;
    }

    Some(path_scope)
}

fn take_shell_token(input: &mut &str) -> Option<String> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        *input = "";
        return None;
    }

    let mut chars = trimmed.chars().peekable();
    let mut token = String::new();
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut consumed = 0usize;

    for ch in chars.by_ref() {
        consumed += ch.len_utf8();
        match ch {
            '\'' | '"' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            ch if in_quotes && ch == quote_char => {
                in_quotes = false;
            }
            ch if !in_quotes && ch.is_whitespace() => {
                break;
            }
            _ => token.push(ch),
        }
    }

    *input = trimmed[consumed..].trim_start();
    if token.is_empty() { None } else { Some(token) }
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let escaped = value.replace('\'', r"'\''");
    format!("'{}'", escaped)
}

fn contains_write_operator(command: &str) -> bool {
    let write_markers = [
        " >",
        ">>",
        " out-file",
        " set-content",
        " add-content",
        " tee-object",
        " remove-item",
        " move-item",
        " copy-item",
        " rename-item",
        " new-item",
        " set-item",
        " rm ",
        " del ",
        " mv ",
        " cp ",
        " chmod ",
        " chown ",
        " mkdir ",
        " rmdir ",
        " sed -i",
    ];
    write_markers.iter().any(|marker| command.contains(marker))
}

fn contains_network_indicator(command: &str) -> bool {
    let network_markers = [
        "curl ",
        "wget ",
        "invoke-webrequest",
        "invoke-restmethod",
        "http://",
        "https://",
        " ping ",
        "ssh ",
        "scp ",
        "ftp ",
        "npm install",
        "pnpm install",
        "yarn install",
        "pip install",
        "cargo install",
    ];
    network_markers
        .iter()
        .any(|marker| command.contains(marker))
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
    use crate::registry::shared::{
        LocalTool, LocalToolInvocation, LocalToolPayload, LocalToolSource,
    };
    use agent_core::ToolExecutionContext;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn command_exists_rejects_missing_binary() {
        assert!(!command_exists("cloudagent-definitely-missing-command"));
    }

    #[test]
    fn summarize_command_classifies_common_read_only_commands() {
        assert_eq!(summarize_command("rg -n TODO src"), "search");
        assert_eq!(summarize_command("git ls-files crates"), "list files");
        assert_eq!(summarize_command("git status"), "inspect");
        assert_eq!(summarize_command("Set-Content out.txt hi"), "action");
    }

    #[test]
    fn rg_search_commands_fall_back_when_rg_is_missing() {
        let translated = translate_rg_command(r#"rg -n "context|token" crates/agent-core"#)
            .expect("search command should translate");
        if cfg!(windows) {
            assert!(translated.contains("Get-ChildItem -Recurse -File"));
            assert!(translated.contains("Select-String -Pattern"));
            assert!(translated.contains("crates/agent-core"));
            assert!(translated.contains("node_modules"));
        } else {
            assert!(translated.starts_with("find 'crates/agent-core'"));
            assert!(translated.contains("-prune"));
            assert!(translated.contains("| xargs -0 grep -n -I"));
        }
    }

    #[test]
    fn rg_files_search_commands_fall_back_when_rg_is_missing() {
        let translated =
            translate_rg_command("rg --files crates").expect("files search should translate");
        if cfg!(windows) {
            assert!(translated.contains("Get-ChildItem -Recurse -File"));
            assert!(translated.contains("-ExpandProperty FullName"));
            assert!(translated.contains("target"));
        } else {
            assert!(translated.starts_with("find 'crates'"));
            assert!(translated.contains("-print0"));
        }
    }

    #[test]
    fn shell_token_parser_handles_quoted_patterns() {
        let mut input = r#"-n "context|token" crates/agent-core"#;
        assert_eq!(take_shell_token(&mut input), Some("-n".to_string()));
        assert_eq!(
            take_shell_token(&mut input),
            Some("context|token".to_string())
        );
        assert_eq!(
            take_shell_token(&mut input),
            Some("crates/agent-core".to_string())
        );
    }

    #[tokio::test]
    async fn exec_command_rejects_apply_patch_style_commands() {
        let tool = ExecCommandLocalTool::new();
        let ctx = ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: std::env::temp_dir(),
            conversation_store_dir: std::env::temp_dir(),
            permission_profile: agent_core::PermissionProfile::ReadOnly,
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        };

        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: agent_core::ToolIdentity::built_in("exec_command"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: serde_json::json!({
                            "command": "apply_patch *** Begin Patch\n*** Update File: src/lib.rs\n*** End Patch"
                        }),
                    },
                },
                &ctx,
            )
            .await
            .expect("exec command handled");

        match output.structured {
            Some(StructuredToolResult::CommandExecution { status, stderr, .. }) => {
                assert_eq!(status, CommandExecutionStatus::Failed);
                assert!(
                    stderr
                        .unwrap_or_default()
                        .contains("Use the dedicated file editing tool instead")
                );
            }
            other => panic!("expected structured command rejection, got {other:?}"),
        }
    }

    #[test]
    fn capped_output_appends_notice() {
        let mut buffer = String::new();
        let mut truncated = false;
        append_capped_chunk(&mut buffer, "abcdef", 4, &mut truncated);
        assert!(truncated);
        assert!(buffer.contains("output truncated"));
    }
}
