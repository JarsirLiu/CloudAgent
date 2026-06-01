use crate::impls::command::output::{
    CommandResultView, DEFAULT_LIVE_OUTPUT_TOKENS_PER_STREAM, capture_token_budget,
    format_exec_result_content, pump_exec_reader, truncate_output_to_tokens,
};
use crate::impls::command::process::build_command_process;
use crate::impls::command::search_fallback::translate_search_command;
use crate::registry::shared::ToolInvocationOutput;
use agent_core::{
    CommandExecutionStatus, StructuredToolResult, ToolExecutionContext, ToolOutputStream,
};
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep, timeout};

#[derive(Default)]
pub(super) struct ExecSessionStore {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<String, ExecSessionEntry>>,
}

impl ExecSessionStore {
    pub(super) fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start_session(
        &self,
        conversation_id: &str,
        command: &str,
        workdir: std::path::PathBuf,
        allow_stdin: bool,
        yield_time_ms: u64,
        max_output_tokens: usize,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let started_at = Instant::now();
        let rendered_command =
            translate_search_command(command).unwrap_or_else(|| command.to_string());
        let mut child = build_command_process(&rendered_command, &workdir, allow_stdin).spawn()?;
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
        let capture_limit_tokens = capture_token_budget(max_output_tokens);
        tokio::spawn(pump_exec_reader(
            stdout,
            ToolOutputStream::Stdout,
            stdout_buffer.clone(),
            ctx.output_tx.clone(),
            capture_limit_tokens,
            DEFAULT_LIVE_OUTPUT_TOKENS_PER_STREAM,
        ));
        tokio::spawn(pump_exec_reader(
            stderr,
            ToolOutputStream::Stderr,
            stderr_buffer.clone(),
            ctx.output_tx.clone(),
            capture_limit_tokens,
            DEFAULT_LIVE_OUTPUT_TOKENS_PER_STREAM,
        ));

        let session = Arc::new(ExecSession {
            command: command.to_string(),
            current_directory: workdir.display().to_string(),
            allow_stdin,
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: stdout_buffer,
            stderr: stderr_buffer,
            stdout_cursor: Mutex::new(0),
            stderr_cursor: Mutex::new(0),
        });
        let session_id = self.allocate_id(conversation_id);
        self.insert(session_id.clone(), session.clone()).await;

        sleep(Duration::from_millis(50)).await;
        let output = build_session_result(
            &session_id,
            session,
            yield_time_ms,
            max_output_tokens,
            started_at,
            ctx,
        )
        .await;
        let output = match output {
            Ok(output) => output,
            Err(err) => {
                let _ = self.cleanup_session(&session_id).await;
                return Err(err);
            }
        };
        if !is_in_progress(&output) {
            let _ = self.cleanup_session(&session_id).await;
        }
        Ok(output)
    }

    pub(super) async fn write_stdin(
        &self,
        session_id: &str,
        chars: &str,
        yield_time_ms: u64,
        max_output_tokens: usize,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let Some(session) = self.get(session_id).await else {
            bail!("exec session `{session_id}` was not found");
        };
        if !chars.is_empty() {
            if !session.allow_stdin {
                bail!(
                    "non-empty stdin is only available in full access mode because interactive input can modify files outside the workspace"
                );
            }
            session.write_stdin(chars).await?;
        }
        let output = build_session_result(
            session_id,
            session.clone(),
            yield_time_ms,
            max_output_tokens,
            Instant::now(),
            ctx,
        )
        .await;
        let output = match output {
            Ok(output) => output,
            Err(err) => {
                let _ = self.cleanup_session(session_id).await;
                return Err(err);
            }
        };
        if !is_in_progress(&output) {
            let _ = self.cleanup_session(session_id).await;
        }
        Ok(output)
    }

    fn allocate_id(&self, conversation_id: &str) -> String {
        let next = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("exec:{}:{next}", conversation_id)
    }

    async fn insert(&self, id: String, session: Arc<ExecSession>) {
        let pruned = {
            let mut sessions = self.sessions.lock().await;
            let pruned = if sessions.len() >= MAX_ACTIVE_EXEC_SESSIONS {
                sessions
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(id, _)| id.clone())
                    .and_then(|oldest_id| sessions.remove(&oldest_id))
            } else {
                None
            };
            sessions.insert(
                id,
                ExecSessionEntry {
                    session,
                    last_used: Instant::now(),
                },
            );
            pruned
        };
        if let Some(entry) = pruned {
            entry.session.terminate().await;
        }
    }

    async fn get(&self, id: &str) -> Option<Arc<ExecSession>> {
        let mut sessions = self.sessions.lock().await;
        let entry = sessions.get_mut(id)?;
        entry.last_used = Instant::now();
        Some(Arc::clone(&entry.session))
    }

    async fn remove(&self, id: &str) -> Option<Arc<ExecSession>> {
        self.sessions
            .lock()
            .await
            .remove(id)
            .map(|entry| entry.session)
    }

    async fn cleanup_session(&self, id: &str) -> Option<Arc<ExecSession>> {
        let session = self.remove(id).await?;
        session.terminate().await;
        Some(session)
    }
}

const MAX_ACTIVE_EXEC_SESSIONS: usize = 16;

struct ExecSessionEntry {
    session: Arc<ExecSession>,
    last_used: Instant,
}

struct ExecSession {
    command: String,
    current_directory: String,
    allow_stdin: bool,
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    stdout_cursor: Mutex<usize>,
    stderr_cursor: Mutex<usize>,
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

    async fn take_new_stdout(&self) -> String {
        take_new_buffer(&self.stdout, &self.stdout_cursor).await
    }

    async fn take_new_stderr(&self) -> String {
        take_new_buffer(&self.stderr, &self.stderr_cursor).await
    }

    async fn terminate(&self) {
        self.close_stdin().await;
        terminate_child_tree(&self.child).await;
    }

    async fn close_stdin(&self) {
        let mut guard = self.stdin.lock().await;
        let _ = guard.take();
    }
}

async fn build_session_result(
    session_id: &str,
    session: Arc<ExecSession>,
    yield_time_ms: u64,
    max_output_tokens: usize,
    started_at: Instant,
    ctx: &ToolExecutionContext,
) -> Result<ToolInvocationOutput> {
    let (status, exit_code, success) =
        wait_for_session(session.as_ref(), yield_time_ms, &ctx.cancellation_token).await?;
    let stdout = session.take_new_stdout().await;
    let stderr = session.take_new_stderr().await;
    let duration_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let raw_output = merge_command_output(&stdout, &stderr);
    let (output, original_token_count) = truncate_output_to_tokens(&raw_output, max_output_tokens);
    let content = format_exec_result_content(CommandResultView {
        command: &session.command,
        current_directory: &session.current_directory,
        session_id: session_id_for_status(session_id, &status),
        status: status.clone(),
        exit_code,
        duration_ms,
        output: &output,
        max_output_tokens,
        original_token_count,
    });
    Ok(ToolInvocationOutput {
        content: content.clone(),
        structured: Some(StructuredToolResult::CommandExecution {
            command: session.command.clone(),
            current_directory: session.current_directory.clone(),
            session_id: session_id_for_status(session_id, &status).map(str::to_string),
            status,
            exit_code,
            success,
            output: Some(output),
            duration_ms: Some(duration_ms),
            original_token_count: Some(original_token_count),
            max_output_tokens: Some(max_output_tokens),
        }),
    })
}

fn merge_command_output(stdout: &str, stderr: &str) -> String {
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("stdout:\n{stdout}\nstderr:\n{stderr}"),
    }
}

fn session_id_for_status<'a>(
    session_id: &'a str,
    status: &CommandExecutionStatus,
) -> Option<&'a str> {
    matches!(status, CommandExecutionStatus::InProgress).then_some(session_id)
}

fn is_in_progress(output: &ToolInvocationOutput) -> bool {
    matches!(
        output.structured,
        Some(StructuredToolResult::CommandExecution {
            status: CommandExecutionStatus::InProgress,
            ..
        })
    )
}

async fn wait_for_session(
    session: &ExecSession,
    timeout_ms: u64,
    cancellation_token: &tokio_util::sync::CancellationToken,
) -> Result<(CommandExecutionStatus, Option<i32>, Option<bool>)> {
    let mut child = session.child.lock().await;
    let exited = tokio::select! {
        _ = cancellation_token.cancelled() => {
            let _ = child.id();
            drop(child);
            session.terminate().await;
            bail!("command aborted by user");
        }
        waited = timeout(Duration::from_millis(timeout_ms), child.wait()) => {
            match waited {
                Ok(result) => Some(result?),
                Err(_) => None,
            }
        }
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

async fn terminate_child_tree(child: &Mutex<Child>) {
    let mut child = child.lock().await;
    if let Some(pid) = child.id() {
        let _ = terminate_child_tree_by_pid(pid).await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn terminate_child_tree_by_pid(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
        let _ = status;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Ok(())
    }
}

async fn take_new_buffer(buffer: &Arc<Mutex<String>>, cursor: &Mutex<usize>) -> String {
    let text = buffer.lock().await.clone();
    let mut cursor = cursor.lock().await;
    let start = (*cursor).min(text.len());
    let out = text[start..].to_string();
    *cursor = text.len();
    out.trim().to_string()
}
