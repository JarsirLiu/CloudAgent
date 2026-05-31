use crate::impls::result_format::{finalize, push_fact, push_section};
use crate::registry::shared::decode_utf8_chunk;
use agent_core::{CommandExecutionStatus, ToolOutputStream};
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::Mutex;

pub(super) const MAX_CAPTURE_CHARS_PER_STREAM: usize = 24_000;
pub(super) const MAX_LIVE_OUTPUT_CHARS_PER_STREAM: usize = 12_000;
const MAX_RESULT_SECTION_CHARS: usize = 6_000;
const OUTPUT_TRUNCATION_NOTICE: &str = "\n[output truncated; narrow the command or use `search_workspace` and follow with `read_file` for repository discovery]\n";

pub(super) struct CommandResultView<'a> {
    pub(super) kind: &'a str,
    pub(super) command: &'a str,
    pub(super) current_directory: &'a str,
    pub(super) session_id: Option<&'a str>,
    pub(super) status: CommandExecutionStatus,
    pub(super) exit_code: Option<i32>,
    pub(super) success: Option<bool>,
    pub(super) stdout: &'a str,
    pub(super) stderr: &'a str,
}

pub(super) fn format_exec_result_content(view: CommandResultView<'_>) -> String {
    let summary = render_command_summary(view.command, &view.status, view.success, view.exit_code);
    let mut lines = Vec::new();
    push_fact(&mut lines, "Kind", view.kind.to_string());
    push_fact(&mut lines, "Command", view.command.to_string());
    push_fact(
        &mut lines,
        "Current directory",
        view.current_directory.to_string(),
    );
    if let Some(session_id) = view.session_id {
        push_fact(&mut lines, "Session", session_id.to_string());
    }
    push_fact(
        &mut lines,
        "Status",
        render_command_status(&view.status).to_string(),
    );
    if let Some(exit_code) = view.exit_code {
        push_fact(&mut lines, "Exit code", exit_code.to_string());
    }
    if let Some(success) = view.success {
        push_fact(&mut lines, "Success", success.to_string());
    }
    push_section(
        &mut lines,
        "Stdout",
        if view.stdout.is_empty() {
            "(empty)".to_string()
        } else {
            compact_result_section(view.stdout)
        },
    );
    push_section(
        &mut lines,
        "Stderr",
        if view.stderr.is_empty() {
            "(empty)".to_string()
        } else {
            compact_result_section(view.stderr)
        },
    );
    let next_step = if view.stdout.contains("output truncated")
        || view.stderr.contains("output truncated")
    {
        Some(
            "narrow the command or use `search_workspace` followed by `read_file` for repository discovery",
        )
    } else if matches!(view.status, CommandExecutionStatus::InProgress) {
        Some("reuse the returned `session_id` to send more stdin or poll for additional output")
    } else {
        None
    };
    finalize(summary, lines, next_step)
}

fn compact_result_section(text: &str) -> String {
    if text.chars().count() <= MAX_RESULT_SECTION_CHARS {
        return text.to_string();
    }

    let head_chars = MAX_RESULT_SECTION_CHARS * 2 / 3;
    let tail_chars = MAX_RESULT_SECTION_CHARS.saturating_sub(head_chars);
    let head = text.chars().take(head_chars).collect::<String>();
    let tail = text
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!(
        "{head}\n[output section compacted; kept head and tail]\n{tail}{OUTPUT_TRUNCATION_NOTICE}"
    )
}

pub(super) async fn pump_exec_reader<R>(
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
            if let Some(output_tx) = &output_tx
                && let Some(delta) = take_live_chunk(
                    &chunk,
                    live_limit_chars,
                    &mut live_chars_sent,
                    &mut live_truncated,
                )
            {
                let _ = output_tx.send(agent_core::ToolOutputDelta {
                    stream: stream.clone(),
                    chunk: delta,
                });
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
        if let Some(output_tx) = &output_tx
            && let Some(delta) = take_live_chunk(
                &tail,
                live_limit_chars,
                &mut live_chars_sent,
                &mut live_truncated,
            )
        {
            let _ = output_tx.send(agent_core::ToolOutputDelta {
                stream,
                chunk: delta,
            });
        }
    }
    Ok(())
}

fn render_command_summary(
    command: &str,
    status: &CommandExecutionStatus,
    success: Option<bool>,
    exit_code: Option<i32>,
) -> String {
    match status {
        CommandExecutionStatus::InProgress => {
            format!("Command is still running: {command}")
        }
        CommandExecutionStatus::Completed => {
            let exit_fragment = exit_code
                .map(|value| format!(" (exit code {value})"))
                .unwrap_or_default();
            format!("Command completed successfully{exit_fragment}: {command}")
        }
        CommandExecutionStatus::Failed => {
            let exit_fragment = exit_code
                .map(|value| format!(" (exit code {value})"))
                .unwrap_or_default();
            let state = if success == Some(false) {
                "failed"
            } else {
                "did not complete successfully"
            };
            format!("Command {state}{exit_fragment}: {command}")
        }
        CommandExecutionStatus::Declined => {
            format!("Command was declined: {command}")
        }
    }
}

fn render_command_status(status: &CommandExecutionStatus) -> &'static str {
    match status {
        CommandExecutionStatus::InProgress => "in_progress",
        CommandExecutionStatus::Completed => "completed",
        CommandExecutionStatus::Failed => "failed",
        CommandExecutionStatus::Declined => "declined",
    }
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

#[cfg(test)]
mod tests {
    use super::{
        CommandResultView, append_capped_chunk, compact_result_section, format_exec_result_content,
    };
    use agent_core::CommandExecutionStatus;

    #[test]
    fn capped_output_appends_notice() {
        let mut buffer = String::new();
        let mut truncated = false;
        append_capped_chunk(&mut buffer, "abcdef", 4, &mut truncated);
        assert!(truncated);
        assert!(buffer.contains("output truncated"));
    }

    #[test]
    fn result_content_compacts_large_sections() {
        let large = "x".repeat(8_000);
        let compacted = compact_result_section(&large);
        assert!(compacted.len() < large.len());
        assert!(compacted.contains("output section compacted"));
    }

    #[test]
    fn command_result_content_does_not_embed_full_large_output() {
        let large = "x".repeat(8_000);
        let content = format_exec_result_content(CommandResultView {
            kind: "action",
            command: "echo many",
            current_directory: ".",
            session_id: None,
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            success: Some(true),
            stdout: &large,
            stderr: "",
        });

        assert!(content.len() < large.len());
        assert!(content.contains("output section compacted"));
    }
}
