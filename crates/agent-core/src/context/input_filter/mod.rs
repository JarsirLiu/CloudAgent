mod adapters;
mod pipeline;

use crate::conversation::ResponseItem;
use crate::tool::StructuredToolResult;
use std::collections::HashMap;

use adapters::git::filter_git_output;
use adapters::install::filter_install_output;
use adapters::rust::filter_rust_build_test_output;
use adapters::tests::filter_test_output;
use pipeline::{filter_failure_tail, filter_tool_output};

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
        output,
        success,
        ..
    }) = structured
    {
        return filter_command_execution_output(command, output.as_deref(), *success);
    }
    filter_tool_output(content)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandInvocation {
    program: String,
    args: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandFamily {
    Git,
    Cargo,
    TestRunner,
    Install,
    Generic,
}

impl CommandInvocation {
    fn parse(command: &str) -> Self {
        let mut parts = command.split_whitespace().map(str::to_ascii_lowercase);
        let program = parts.next().unwrap_or_default();
        let args = parts.collect();
        Self { program, args }
    }

    fn first_non_option_arg(&self) -> Option<&str> {
        self.args
            .iter()
            .map(String::as_str)
            .find(|arg| !arg.starts_with('-') && !arg.starts_with('+'))
    }

    fn has_passthrough_flag(&self) -> bool {
        self.args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--verbose" | "-vv" | "--nocapture" | "--full"
            )
        })
    }

    fn family(&self) -> CommandFamily {
        match self.program.as_str() {
            "git" => CommandFamily::Git,
            "cargo" => match self.first_non_option_arg() {
                Some("install") => CommandFamily::Install,
                Some("test") | Some("build") => CommandFamily::Cargo,
                _ => CommandFamily::Generic,
            },
            "pytest" | "py.test" => CommandFamily::TestRunner,
            "python" | "python3" => {
                if self
                    .args
                    .windows(2)
                    .any(|window| window[0] == "-m" && window[1] == "pytest")
                {
                    CommandFamily::TestRunner
                } else {
                    CommandFamily::Generic
                }
            }
            "npm" | "pnpm" => match self.first_non_option_arg() {
                Some("test") => CommandFamily::TestRunner,
                Some("install") => CommandFamily::Install,
                _ => CommandFamily::Generic,
            },
            _ => CommandFamily::Generic,
        }
    }
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
        StructuredToolResult::ReadFile {
            path,
            start_line,
            max_lines,
            ..
        } => Some(format!(
            "{tool_name}:{path}:{:?}:{:?}",
            start_line, max_lines
        )),
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
        StructuredToolResult::ReadFile {
            path,
            total_chars,
            read,
            ..
        } => format!(
            "[rtk:{tool_name}]\ntool: {tool_name}\npath: {path}\nstatus: superseded by a newer read\ntruncated: {}\nnext_start_line: {}\ntotal_chars: {total_chars}",
            read.truncated,
            read.next_start_line
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        StructuredToolResult::GetMetadata {
            path,
            exists,
            is_file,
            is_dir,
            is_symlink,
            size,
            readonly,
            created_at_ms,
            modified_at_ms,
        } => format!(
            "[rtk:get_metadata]\npath: {path}\nstatus: superseded by a newer metadata lookup\nexists: {exists}\nis_file: {is_file}\nis_dir: {is_dir}\nis_symlink: {is_symlink}\nsize: {size}\nreadonly: {readonly}\ncreated_at_ms: {}\nmodified_at_ms: {}",
            created_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            modified_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
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
    output: Option<&str>,
    success: Option<bool>,
) -> String {
    let invocation = CommandInvocation::parse(command.trim());
    let merged = output.unwrap_or_default();
    if invocation.has_passthrough_flag() {
        return merged.to_string();
    }
    match invocation.family() {
        CommandFamily::Git => {
            let normalized_command = normalized_command_line(&invocation);
            wrap_summary("git", &filter_git_output(&normalized_command, merged), merged)
        }
        CommandFamily::Cargo => wrap_summary("cargo", &filter_rust_build_test_output(merged), merged),
        CommandFamily::TestRunner => wrap_summary("test", &filter_test_output(merged), merged),
        CommandFamily::Install => wrap_summary("install", &filter_install_output(merged), merged),
        CommandFamily::Generic => {
            if success == Some(false) {
                return wrap_summary("fallback", &filter_failure_tail(merged), merged);
            }
            wrap_summary("generic", &filter_tool_output(merged), merged)
        }
    }
}

fn wrap_summary(kind: &str, filtered: &str, raw: &str) -> String {
    // Stable template improves behavior consistency and cache reuse.
    let stable = format!("[rtk:{kind}]\n{filtered}");
    if stable.trim().is_empty() || stable.lines().count() < 2 {
        return raw.to_string();
    }
    stable
}

fn normalized_command_line(invocation: &CommandInvocation) -> String {
    let mut parts = Vec::with_capacity(invocation.args.len() + 1);
    parts.push(invocation.program.clone());
    parts.extend(invocation.args.iter().cloned());
    parts.join(" ")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
