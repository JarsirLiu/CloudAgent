mod adapters;
mod pipeline;

use crate::conversation::ResponseItem;
use crate::tool::StructuredToolResult;

use adapters::git::filter_git_output;
use adapters::install::filter_install_output;
use adapters::rust::filter_rust_build_test_output;
use adapters::tests::filter_test_output;
use pipeline::{filter_failure_tail, filter_tool_output, merge_streams};

#[derive(Clone, Debug, Default)]
pub struct ContextInputFilterService;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterPolicy {
    pub enabled: bool,
}

impl ContextInputFilterService {
    pub fn new() -> Self {
        Self
    }

    pub fn filter_for_model(
        &self,
        messages: Vec<ResponseItem>,
        policy: FilterPolicy,
    ) -> Vec<ResponseItem> {
        if !policy.enabled {
            return messages;
        }
        messages
            .into_iter()
            .map(|item| match item {
                ResponseItem::Tool {
                    tool_call_id,
                    name,
                    content,
                    structured,
                } => ResponseItem::Tool {
                    tool_call_id,
                    name,
                    content: filter_tool_output_for_item(&content, structured.as_ref()),
                    structured,
                },
                other => other,
            })
            .collect()
    }
}

fn filter_tool_output_for_item(content: &str, structured: Option<&StructuredToolResult>) -> String {
    if let Some(StructuredToolResult::CommandExecution {
        command,
        stdout,
        stderr,
        success,
        ..
    }) = structured
    {
        return filter_command_execution_output(command, stdout.as_deref(), stderr.as_deref(), *success);
    }
    filter_tool_output(content)
}

fn filter_command_execution_output(
    command: &str,
    stdout: Option<&str>,
    stderr: Option<&str>,
    success: Option<bool>,
) -> String {
    let cmd = command.trim().to_ascii_lowercase();
    let merged = merge_streams(stdout, stderr);
    if should_passthrough(&cmd) {
        return merged;
    }
    if cmd.starts_with("git ") {
        return wrap_summary("git", &filter_git_output(&cmd, &merged), &merged);
    }
    if cmd.contains("cargo test") || cmd.contains("cargo build") {
        return wrap_summary("cargo", &filter_rust_build_test_output(&merged), &merged);
    }
    if cmd.contains("pytest") || cmd.contains("npm test") || cmd.contains("pnpm test") {
        return wrap_summary("test", &filter_test_output(&merged), &merged);
    }
    if cmd.contains("npm install")
        || cmd.contains("pnpm install")
        || cmd.contains("cargo install")
    {
        return wrap_summary("install", &filter_install_output(&merged), &merged);
    }
    if success == Some(false) {
        return wrap_summary("fallback", &filter_failure_tail(&merged), &merged);
    }
    wrap_summary("generic", &filter_tool_output(&merged), &merged)
}

fn should_passthrough(cmd: &str) -> bool {
    // Respect explicit detail requests where compression can remove needed evidence.
    cmd.contains("--verbose")
        || cmd.contains("-vv")
        || cmd.contains("--nocapture")
        || cmd.contains("--full")
}

fn wrap_summary(kind: &str, filtered: &str, raw: &str) -> String {
    // Stable template improves behavior consistency and cache reuse.
    let stable = format!("[rtk:{kind}]\n{filtered}");
    if stable.trim().is_empty() || stable.lines().count() < 2 {
        return raw.to_string();
    }
    stable
}

#[cfg(test)]
fn run_filter(
    command: &str,
    stdout: Option<&str>,
    stderr: Option<&str>,
    success: Option<bool>,
) -> String {
    filter_command_execution_output(command, stdout, stderr, success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::StructuredToolResult;

    #[test]
    fn filter_disabled_keeps_original_message() {
        let svc = ContextInputFilterService::new();
        let input = vec![ResponseItem::User {
            content: "hello".to_string(),
        }];
        let out = svc.filter_for_model(input.clone(), FilterPolicy { enabled: false });
        assert_eq!(out.len(), 1);
        match &out[0] {
            ResponseItem::User { content } => assert_eq!(content, "hello"),
            _ => panic!("expected user message"),
        }
    }

    #[test]
    fn git_diff_is_summarized() {
        let structured = StructuredToolResult::CommandExecution {
            command: "git diff".to_string(),
            current_directory: "D:\\repo".to_string(),
            status: crate::tool::CommandExecutionStatus::Completed,
            exit_code: Some(0),
            success: Some(true),
            stdout: Some("+++ a\n--- b\n+line\n-line\n".to_string()),
            stderr: None,
            aggregated_output: None,
            duration_ms: Some(1),
        };
        let svc = ContextInputFilterService::new();
        let out = svc.filter_for_model(
            vec![ResponseItem::Tool {
                tool_call_id: "1".to_string(),
                name: "shell_command".to_string(),
                content: "raw".to_string(),
                structured: Some(structured),
            }],
            FilterPolicy { enabled: true },
        );
        match &out[0] {
            ResponseItem::Tool { content, .. } => assert!(content.contains("Git diff summary")),
            _ => panic!("expected tool message"),
        }
    }

    #[test]
    fn install_output_is_compacted() {
        let text = "Downloading x\nadded 10 packages\naudited 10 packages\n";
        let compacted = filter_install_output(text);
        assert!(compacted.contains("Install summary"));
        assert!(compacted.contains("added 10 packages"));
    }

    #[test]
    fn passthrough_verbose_command() {
        let out = run_filter("cargo test -- --nocapture", Some("full output"), None, Some(true));
        assert_eq!(out, "full output");
    }

    #[test]
    fn generic_output_has_stable_header() {
        let out = run_filter("echo hi", Some("line1\nline2"), None, Some(true));
        assert!(out.starts_with("[rtk:generic]"));
    }

    #[test]
    fn failed_command_keeps_tail_with_header() {
        let out = run_filter("unknown", Some("a\nb\nc"), Some("err1\nerr2"), Some(false));
        assert!(out.starts_with("[rtk:fallback]"));
        assert!(out.contains("err2"));
    }

    #[test]
    fn git_status_uses_git_header() {
        let raw = "modified: a.rs\nnew file: b.rs";
        let out = run_filter("git status", Some(raw), None, Some(true));
        assert!(out.starts_with("[rtk:git]"));
        assert!(out.contains("changed files"));
    }

    #[test]
    fn test_runner_uses_test_header() {
        let raw = "PASSED t1\nFAILED t2";
        let out = run_filter("pytest -q", Some(raw), None, Some(false));
        assert!(out.starts_with("[rtk:test]"));
        assert!(out.contains("Test summary"));
    }
}
