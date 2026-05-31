use crate::impls::result_format::{finalize, push_fact, push_section};
use crate::registry::shared::decode_utf8_chunk;
use agent_core::output_truncation::{
    DEFAULT_MAX_OUTPUT_TOKENS, approximate_token_count, truncate_text_to_token_budget,
};
use agent_core::{CommandExecutionStatus, ToolOutputStream};
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::Mutex;

const CAPTURE_TOKEN_HEADROOM_MULTIPLIER: usize = 2;
pub(super) const DEFAULT_LIVE_OUTPUT_TOKENS_PER_STREAM: usize = 4_000;
const OUTPUT_TRUNCATION_NOTICE: &str = "\n[output truncated; narrow the command, tighten the `rg` pattern, or read a smaller file slice]\n";

pub(super) struct CommandResultView<'a> {
    pub(super) command: &'a str,
    pub(super) current_directory: &'a str,
    pub(super) session_id: Option<&'a str>,
    pub(super) status: CommandExecutionStatus,
    pub(super) exit_code: Option<i32>,
    pub(super) duration_ms: u64,
    pub(super) output: &'a str,
    pub(super) max_output_tokens: usize,
    pub(super) original_token_count: usize,
}

pub(super) fn format_exec_result_content(view: CommandResultView<'_>) -> String {
    let summary = render_command_summary(view.command, &view.status, view.exit_code);
    let mut lines = Vec::new();
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
    push_fact(
        &mut lines,
        "Wall time seconds",
        format!("{:.3}", view.duration_ms as f64 / 1000.0),
    );
    if view.original_token_count > view.max_output_tokens {
        push_fact(
            &mut lines,
            "Original token count",
            view.original_token_count.to_string(),
        );
    }
    push_section(
        &mut lines,
        "Output",
        if view.output.is_empty() {
            "(empty)".to_string()
        } else {
            view.output.to_string()
        },
    );
    let next_step = if view.original_token_count > view.max_output_tokens {
        Some("narrow the command, tighten the `rg` pattern, or read a smaller file slice")
    } else if matches!(view.status, CommandExecutionStatus::InProgress) {
        Some("reuse the returned `session_id` to send more stdin or poll for additional output")
    } else {
        None
    };
    finalize(summary, lines, next_step)
}

pub(super) fn resolve_max_output_tokens(value: Option<usize>) -> usize {
    value.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS).max(1)
}

pub(super) fn effective_max_output_tokens(requested: Option<usize>, policy_limit: usize) -> usize {
    resolve_max_output_tokens(requested).min(policy_limit.max(1))
}

pub(super) fn capture_token_budget(max_output_tokens: usize) -> usize {
    max_output_tokens
        .saturating_mul(CAPTURE_TOKEN_HEADROOM_MULTIPLIER)
        .max(1)
}

pub(super) fn truncate_output_to_tokens(text: &str, max_tokens: usize) -> (String, usize) {
    let truncated = truncate_text_to_token_budget(text, max_tokens, Some(OUTPUT_TRUNCATION_NOTICE));
    (truncated.text, truncated.original_token_count)
}

pub(super) async fn pump_exec_reader<R>(
    mut reader: R,
    stream: ToolOutputStream,
    buffer: Arc<Mutex<String>>,
    output_tx: Option<tokio::sync::mpsc::UnboundedSender<agent_core::ToolOutputDelta>>,
    capture_limit_tokens: usize,
    live_limit_tokens: usize,
) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut raw = [0_u8; 8192];
    let mut pending_utf8 = Vec::new();
    let mut capture_truncated = false;
    let mut live_truncated = false;
    let mut live_tokens_sent = 0usize;
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
                capture_limit_tokens,
                &mut capture_truncated,
            );
            if let Some(output_tx) = &output_tx
                && let Some(delta) = take_live_chunk(
                    &chunk,
                    live_limit_tokens,
                    &mut live_tokens_sent,
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
            capture_limit_tokens,
            &mut capture_truncated,
        );
        if let Some(output_tx) = &output_tx
            && let Some(delta) = take_live_chunk(
                &tail,
                live_limit_tokens,
                &mut live_tokens_sent,
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
            let state = "failed";
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

fn append_capped_chunk(
    buffer: &mut String,
    chunk: &str,
    limit_tokens: usize,
    truncated: &mut bool,
) {
    if *truncated {
        return;
    }
    let current_tokens = approximate_token_count(buffer);
    if current_tokens >= limit_tokens {
        buffer.push_str(OUTPUT_TRUNCATION_NOTICE);
        *truncated = true;
        return;
    }
    let remaining_tokens = limit_tokens.saturating_sub(current_tokens);
    let chunk_tokens = approximate_token_count(chunk);
    if chunk_tokens <= remaining_tokens {
        buffer.push_str(chunk);
        return;
    }
    let remaining_chars = remaining_tokens.saturating_mul(3).max(1);
    buffer.push_str(&chunk.chars().take(remaining_chars).collect::<String>());
    buffer.push_str(OUTPUT_TRUNCATION_NOTICE);
    *truncated = true;
}

fn take_live_chunk(
    chunk: &str,
    limit_tokens: usize,
    live_tokens_sent: &mut usize,
    truncated: &mut bool,
) -> Option<String> {
    if *truncated {
        return None;
    }
    if *live_tokens_sent >= limit_tokens {
        *truncated = true;
        return Some(OUTPUT_TRUNCATION_NOTICE.to_string());
    }
    let remaining_tokens = limit_tokens.saturating_sub(*live_tokens_sent);
    let chunk_tokens = approximate_token_count(chunk);
    if chunk_tokens <= remaining_tokens {
        *live_tokens_sent += chunk_tokens;
        return Some(chunk.to_string());
    }
    let remaining_chars = remaining_tokens.saturating_mul(3).max(1);
    let mut rendered = chunk.chars().take(remaining_chars).collect::<String>();
    rendered.push_str(OUTPUT_TRUNCATION_NOTICE);
    *live_tokens_sent = limit_tokens;
    *truncated = true;
    Some(rendered)
}

#[cfg(test)]
mod tests {
    use super::{
        CommandResultView, append_capped_chunk, format_exec_result_content,
        truncate_output_to_tokens,
    };
    use agent_core::CommandExecutionStatus;

    #[test]
    fn capped_output_appends_notice() {
        let mut buffer = String::new();
        let mut truncated = false;
        append_capped_chunk(&mut buffer, "abcdef", 1, &mut truncated);
        assert!(truncated);
        assert!(buffer.contains("output truncated"));
    }

    #[test]
    fn result_content_truncates_to_token_budget() {
        let large = "x".repeat(30_000);
        let (compacted, original_token_count) = truncate_output_to_tokens(&large, 1_000);
        assert!(compacted.len() < large.len());
        assert!(compacted.contains("tokens truncated"));
        assert!(original_token_count > 1_000);
    }

    #[test]
    fn command_result_content_does_not_embed_full_large_output() {
        let large = "x".repeat(8_000);
        let (output, original_token_count) = truncate_output_to_tokens(&large, 1_000);
        let content = format_exec_result_content(CommandResultView {
            command: "echo many",
            current_directory: ".",
            session_id: None,
            status: CommandExecutionStatus::Completed,
            exit_code: Some(0),
            duration_ms: 1,
            output: &output,
            max_output_tokens: 1_000,
            original_token_count,
        });

        assert!(content.len() < large.len());
        assert!(content.contains("Original token count"));
    }
}
