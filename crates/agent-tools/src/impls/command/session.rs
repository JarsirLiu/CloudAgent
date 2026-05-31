use crate::command_access::classify_command;
use crate::impls::command::output::{
    CommandResultView, MAX_CAPTURE_CHARS_PER_STREAM, MAX_LIVE_OUTPUT_CHARS_PER_STREAM,
    format_exec_result_content, pump_exec_reader,
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
    sessions: Mutex<HashMap<String, Arc<ExecSession>>>,
}

impl ExecSessionStore {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) async fn start_session(
        &self,
        conversation_id: &str,
        command: &str,
        workdir: std::path::PathBuf,
        allow_stdin: bool,
        timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let started_at = Instant::now();
        let rendered_command =
            translate_search_command(command).unwrap_or_else(|| command.to_string());
        let mut child = build_command_process(&rendered_command, &workdir).spawn()?;
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
        tokio::spawn(pump_exec_reader(
            stdout,
            ToolOutputStream::Stdout,
            stdout_buffer.clone(),
            ctx.output_tx.clone(),
            MAX_CAPTURE_CHARS_PER_STREAM,
            MAX_LIVE_OUTPUT_CHARS_PER_STREAM,
        ));
        tokio::spawn(pump_exec_reader(
            stderr,
            ToolOutputStream::Stderr,
            stderr_buffer.clone(),
            ctx.output_tx.clone(),
            MAX_CAPTURE_CHARS_PER_STREAM,
            MAX_LIVE_OUTPUT_CHARS_PER_STREAM,
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
        let output =
            build_session_result(&session_id, session, timeout_ms, started_at, ctx).await?;
        if !is_in_progress(&output) {
            let _ = self.remove(&session_id).await;
        }
        Ok(output)
    }

    pub(super) async fn write_stdin(
        &self,
        session_id: &str,
        chars: &str,
        timeout_ms: u64,
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
        let output =
            build_session_result(session_id, session.clone(), timeout_ms, Instant::now(), ctx)
                .await?;
        if !is_in_progress(&output) {
            let _ = self.remove(session_id).await;
        }
        Ok(output)
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
}

async fn build_session_result(
    session_id: &str,
    session: Arc<ExecSession>,
    timeout_ms: u64,
    started_at: Instant,
    ctx: &ToolExecutionContext,
) -> Result<ToolInvocationOutput> {
    let (status, exit_code, success) =
        wait_for_session(&session.child, timeout_ms, &ctx.cancellation_token).await?;
    let stdout = session.take_new_stdout().await;
    let stderr = session.take_new_stderr().await;
    let duration_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let content = format_exec_result_content(CommandResultView {
        kind: classify_command(&session.command).summary(&session.command),
        command: &session.command,
        current_directory: &session.current_directory,
        session_id: session_id_for_status(session_id, &status),
        status: status.clone(),
        exit_code,
        success,
        stdout: &stdout,
        stderr: &stderr,
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
            stdout: Some(stdout),
            stderr: Some(stderr),
            aggregated_output: Some(content),
            duration_ms: Some(duration_ms),
        }),
    })
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
    child: &Mutex<Child>,
    timeout_ms: u64,
    cancellation_token: &tokio_util::sync::CancellationToken,
) -> Result<(CommandExecutionStatus, Option<i32>, Option<bool>)> {
    let mut child = child.lock().await;
    let exited = tokio::select! {
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

async fn take_new_buffer(buffer: &Arc<Mutex<String>>, cursor: &Mutex<usize>) -> String {
    let text = buffer.lock().await.clone();
    let mut cursor = cursor.lock().await;
    let start = (*cursor).min(text.len());
    let out = text[start..].to_string();
    *cursor = text.len();
    out.trim().to_string()
}
