mod adapters;
mod pipeline;

use crate::conversation::ResponseItem;
use crate::tool::StructuredToolResult;
use std::collections::HashMap;

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
        let latest_dedupe_indexes = collect_latest_dedupe_indexes(&messages);
        messages
            .into_iter()
            .enumerate()
            .map(|(index, item)| match item {
                ResponseItem::Tool {
                    tool_call_id,
                    name,
                    content,
                    structured,
                } => {
                    let filtered_content = filter_tool_output_for_item(
                        index,
                        &name,
                        &content,
                        structured.as_ref(),
                        &latest_dedupe_indexes,
                    );
                    ResponseItem::Tool {
                        tool_call_id,
                        name,
                        content: filtered_content,
                        structured,
                    }
                }
                other => other,
            })
            .collect()
    }
}

fn filter_tool_output_for_item(
    index: usize,
    tool_name: &str,
    content: &str,
    structured: Option<&StructuredToolResult>,
    latest_dedupe_indexes: &HashMap<String, usize>,
) -> String {
    if let Some(summary) = structured.and_then(|structured| {
        summarize_superseded_tool_result(index, tool_name, structured, latest_dedupe_indexes)
    }) {
        return summary;
    }
    if let Some(StructuredToolResult::CommandExecution {
        command,
        stdout,
        stderr,
        success,
        ..
    }) = structured
    {
        return filter_command_execution_output(
            command,
            stdout.as_deref(),
            stderr.as_deref(),
            *success,
        );
    }
    filter_tool_output(content)
}

fn collect_latest_dedupe_indexes(messages: &[ResponseItem]) -> HashMap<String, usize> {
    let mut latest = HashMap::new();
    for (index, item) in messages.iter().enumerate() {
        let ResponseItem::Tool {
            name,
            structured: Some(structured),
            ..
        } = item
        else {
            continue;
        };
        if let Some(key) = dedupe_key(name, structured) {
            latest.insert(key, index);
        }
    }
    latest
}

fn summarize_superseded_tool_result(
    index: usize,
    tool_name: &str,
    structured: &StructuredToolResult,
    latest_dedupe_indexes: &HashMap<String, usize>,
) -> Option<String> {
    let key = dedupe_key(tool_name, structured)?;
    let latest_index = *latest_dedupe_indexes.get(&key)?;
    if latest_index == index {
        return None;
    }
    Some(render_superseded_summary(tool_name, structured))
}

fn dedupe_key(tool_name: &str, structured: &StructuredToolResult) -> Option<String> {
    match structured {
        StructuredToolResult::ReadFiles {
            paths,
            start_line,
            max_lines,
            ..
        } => Some(format!(
            "read_files:{tool_name}:{:?}:{:?}:{:?}",
            paths, start_line, max_lines
        )),
        StructuredToolResult::ListDirectory {
            path,
            recursive,
            offset,
            ..
        } => Some(format!("list_directory:{path}:{recursive}:{offset}")),
        StructuredToolResult::GetMetadata { path, .. } => Some(format!("metadata:{path}")),
        StructuredToolResult::SearchWorkspace {
            operation,
            mode,
            query,
            path_scope,
            case_sensitive,
            context_lines,
            offset,
            max_results,
            ..
        } => Some(format!(
            "search_workspace:{tool_name}:{operation:?}:{mode:?}:{query}:{:?}:{case_sensitive}:{context_lines}:{offset}:{max_results}",
            path_scope,
        )),
        _ => None,
    }
}

fn render_superseded_summary(tool_name: &str, structured: &StructuredToolResult) -> String {
    match structured {
        StructuredToolResult::ReadFiles {
            paths,
            file_count,
            failed_count,
            truncated_count,
            total_chars,
            ..
        } => format!(
            "[rtk:read_files]\ntool: {tool_name}\npaths: {}\nstatus: superseded by a newer read\nfiles: {file_count}\nfailed_files: {failed_count}\ntruncated_files: {truncated_count}\ntotal_chars: {total_chars}",
            paths.join(", ")
        ),
        StructuredToolResult::ListDirectory {
            path,
            recursive,
            shown_count,
            total_count,
            truncated,
            ..
        } => format!(
            "[rtk:list_directory]\npath: {path}\nrecursive: {recursive}\nstatus: superseded by a newer directory listing\nshown: {shown_count}\ntotal: {total_count}\ntruncated: {truncated}"
        ),
        StructuredToolResult::GetMetadata {
            path,
            exists,
            is_file,
            is_dir,
            size,
            readonly,
        } => format!(
            "[rtk:get_metadata]\npath: {path}\nstatus: superseded by a newer metadata lookup\nexists: {exists}\nis_file: {is_file}\nis_dir: {is_dir}\nsize: {size}\nreadonly: {readonly}"
        ),
        StructuredToolResult::SearchWorkspace {
            session_id,
            operation,
            mode,
            status,
            query,
            path_scope,
            case_sensitive,
            context_lines,
            offset,
            max_results,
            match_count,
            file_count,
            truncated,
            next_offset,
            ..
        } => format!(
            "[rtk:search_workspace]\ntool: {tool_name}\nsession_id: {session_id}\noperation: {operation:?}\nmode: {mode:?}\nstatus: {status:?}\nquery: {query}\npath_scope: {}\ncase_sensitive: {case_sensitive}\ncontext_lines: {context_lines}\noffset: {offset}\nmax_results: {max_results}\nnext_offset: {}\nstatus_detail: superseded by newer search results\nfiles: {file_count}\nmatches: {match_count}\ntruncated: {truncated}",
            next_offset
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            path_scope.as_deref().unwrap_or(".")
        ),
        _ => "(no significant output)".to_string(),
    }
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
    if cmd.contains("npm install") || cmd.contains("pnpm install") || cmd.contains("cargo install")
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
            session_id: None,
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
                name: "exec_command".to_string(),
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
        let out = run_filter(
            "cargo test -- --nocapture",
            Some("full output"),
            None,
            Some(true),
        );
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

    #[test]
    fn older_duplicate_read_file_is_compressed_but_latest_stays_raw() {
        let svc = ContextInputFilterService::new();
        let messages = vec![
            ResponseItem::Tool {
                tool_call_id: "1".to_string(),
                name: "read_files".to_string(),
                content: "old file body".to_string(),
                structured: Some(StructuredToolResult::ReadFiles {
                    paths: vec!["src/app.rs".to_string()],
                    start_line: None,
                    max_lines: None,
                    file_count: 1,
                    failed_count: 0,
                    truncated_count: 0,
                    total_chars: 120,
                    reads: Vec::new(),
                }),
            },
            ResponseItem::Tool {
                tool_call_id: "2".to_string(),
                name: "read_files".to_string(),
                content: "new file body".to_string(),
                structured: Some(StructuredToolResult::ReadFiles {
                    paths: vec!["src/app.rs".to_string()],
                    start_line: None,
                    max_lines: None,
                    file_count: 1,
                    failed_count: 0,
                    truncated_count: 0,
                    total_chars: 160,
                    reads: Vec::new(),
                }),
            },
        ];

        let out = svc.filter_for_model(messages, FilterPolicy { enabled: true });
        match &out[0] {
            ResponseItem::Tool { content, .. } => {
                assert!(content.starts_with("[rtk:read_files]"));
                assert!(content.contains("superseded by a newer read"));
            }
            _ => panic!("expected tool message"),
        }
        match &out[1] {
            ResponseItem::Tool { content, .. } => assert_eq!(content, "new file body"),
            _ => panic!("expected tool message"),
        }
    }

    #[test]
    fn older_duplicate_find_files_is_compressed_only_for_same_query() {
        let svc = ContextInputFilterService::new();
        let messages = vec![
            ResponseItem::Tool {
                tool_call_id: "1".to_string(),
                name: "search_workspace".to_string(),
                content: "Top 2 matches:\nsrc/app.rs\nsrc/lib.rs".to_string(),
                structured: Some(StructuredToolResult::SearchWorkspace {
                    session_id: "search:test:1".to_string(),
                    operation: crate::tool::SearchWorkspaceOperation::Search,
                    mode: crate::tool::SearchWorkspaceMode::Files,
                    status: crate::tool::SearchWorkspaceStatus::Active,
                    query: "app".to_string(),
                    path_scope: Some("src".to_string()),
                    case_sensitive: false,
                    context_lines: 0,
                    offset: 0,
                    max_results: 20,
                    file_count: 2,
                    match_count: 0,
                    truncated: false,
                    next_offset: None,
                    hits: Vec::new(),
                }),
            },
            ResponseItem::Tool {
                tool_call_id: "2".to_string(),
                name: "search_workspace".to_string(),
                content: "Top 3 matches:\nsrc/app.rs\nsrc/lib.rs\nsrc/main.rs".to_string(),
                structured: Some(StructuredToolResult::SearchWorkspace {
                    session_id: "search:test:1".to_string(),
                    operation: crate::tool::SearchWorkspaceOperation::Search,
                    mode: crate::tool::SearchWorkspaceMode::Files,
                    status: crate::tool::SearchWorkspaceStatus::Active,
                    query: "app".to_string(),
                    path_scope: Some("src".to_string()),
                    case_sensitive: false,
                    context_lines: 0,
                    offset: 0,
                    max_results: 20,
                    file_count: 3,
                    match_count: 0,
                    truncated: false,
                    next_offset: None,
                    hits: Vec::new(),
                }),
            },
            ResponseItem::Tool {
                tool_call_id: "3".to_string(),
                name: "search_workspace".to_string(),
                content: "Top 1 matches:\nREADME.md".to_string(),
                structured: Some(StructuredToolResult::SearchWorkspace {
                    session_id: "search:test:2".to_string(),
                    operation: crate::tool::SearchWorkspaceOperation::Search,
                    mode: crate::tool::SearchWorkspaceMode::Files,
                    status: crate::tool::SearchWorkspaceStatus::Active,
                    query: "readme".to_string(),
                    path_scope: None,
                    case_sensitive: false,
                    context_lines: 0,
                    offset: 0,
                    max_results: 20,
                    file_count: 1,
                    match_count: 0,
                    truncated: false,
                    next_offset: None,
                    hits: Vec::new(),
                }),
            },
        ];

        let out = svc.filter_for_model(messages, FilterPolicy { enabled: true });
        match &out[0] {
            ResponseItem::Tool { content, .. } => {
                assert!(content.starts_with("[rtk:search_workspace]"));
                assert!(content.contains("query: app"));
                assert!(content.contains("superseded by newer search results"));
            }
            _ => panic!("expected tool message"),
        }
        match &out[1] {
            ResponseItem::Tool { content, .. } => assert!(content.starts_with("Top 3 matches:")),
            _ => panic!("expected tool message"),
        }
        match &out[2] {
            ResponseItem::Tool { content, .. } => assert!(content.starts_with("Top 1 matches:")),
            _ => panic!("expected tool message"),
        }
    }
}
