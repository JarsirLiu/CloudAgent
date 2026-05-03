use agent_core::{ToolCall, ToolSpec};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ApprovalRequirement {
    pub(crate) requires_approval: bool,
    pub(crate) reason: Option<String>,
}

impl ApprovalRequirement {
    pub(crate) fn not_required() -> Self {
        Self {
            requires_approval: false,
            reason: None,
        }
    }

    pub(crate) fn required(reason: impl Into<String>) -> Self {
        Self {
            requires_approval: true,
            reason: Some(reason.into()),
        }
    }
}

pub(crate) fn approval_requirement_for_tool(
    spec: &ToolSpec,
    call: &ToolCall,
    workspace_root: &Path,
    permission_mode: &str,
) -> ApprovalRequirement {
    match call.name.as_str() {
        "shell_command" => shell_command_requirement(call, workspace_root, permission_mode),
        _ if spec.requires_approval => ApprovalRequirement::required(
            spec.approval_reason
                .clone()
                .unwrap_or_else(|| format!("Tool `{}` requires approval.", call.name)),
        ),
        _ => ApprovalRequirement::not_required(),
    }
}

#[derive(Deserialize)]
struct ShellCommandArgs {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
}

fn shell_command_requirement(
    call: &ToolCall,
    _workspace_root: &Path,
    permission_mode: &str,
) -> ApprovalRequirement {
    let Ok(args) = serde_json::from_value::<ShellCommandArgs>(call.arguments.clone()) else {
        return ApprovalRequirement::required(
            "Shell commands require approval when their arguments cannot be classified safely.",
        );
    };

    let command = args.command.trim();
    if command.is_empty() {
        return ApprovalRequirement::required("Empty shell commands require approval.");
    }

    let normalized = normalize_command(command);
    let readonly = is_safe_readonly_command(command);
    let workdir_escape = workdir_mentions_parent_escape(args.workdir.as_deref());
    let dangerous = is_dangerous_command(&normalized);
    let network = contains_network_indicator(&normalized);
    let mutating = contains_write_operator(&normalized) || !readonly;

    match permission_mode {
        "danger" => {
            if dangerous {
                ApprovalRequirement::required(
                    "Dangerous shell commands (e.g. rm -rf / recursive delete) require approval even in danger mode.",
                )
            } else {
                ApprovalRequirement::not_required()
            }
        }
        "balanced" => {
            if dangerous {
                return ApprovalRequirement::required(
                    "Dangerous shell commands require approval in balanced mode.",
                );
            }
            if workdir_escape {
                return ApprovalRequirement::required(
                    "Balanced mode only allows read/write inside the workspace.",
                );
            }
            if network {
                return ApprovalRequirement::required(
                    "Network shell commands require approval in balanced mode.",
                );
            }
            if mutating || readonly {
                return ApprovalRequirement::not_required();
            }
            ApprovalRequirement::required("Unclassified shell commands require approval.")
        }
        _ => {
            if readonly && !workdir_escape {
                ApprovalRequirement::not_required()
            } else {
                ApprovalRequirement::required(
                    "Safe mode is read-only. Mutating, out-of-workspace, network, or unclassified shell commands require approval.",
                )
            }
        }
    }
}

fn workdir_mentions_parent_escape(workdir: Option<&str>) -> bool {
    workdir.is_some_and(|value| {
        let trimmed = value.trim();
        trimmed.starts_with("..") || trimmed.contains("\\..\\") || trimmed.contains("/../")
    })
}


fn is_safe_readonly_command(command: &str) -> bool {
    let normalized = normalize_command(command);
    if normalized.is_empty() {
        return false;
    }

    if contains_write_operator(&normalized) || contains_network_indicator(&normalized) {
        return false;
    }

    if is_safe_readonly_chain(&normalized) {
        return true;
    }

    let Some(program) = normalized.split_whitespace().next() else {
        return false;
    };

    match program {
        "pwd" | "ls" | "dir" | "cat" | "type" | "head" | "tail" | "find" | "tree" | "rg" | "fd"
        | "findstr" | "select-string" | "get-childitem" | "get-content" | "measure-object"
        | "where-object" | "sort-object" | "select-object" => true,
        "git" => is_safe_git_command(&normalized),
        _ => false,
    }
}

fn is_safe_readonly_chain(command: &str) -> bool {
    if command.contains("&&") {
        return command
            .split("&&")
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .all(is_safe_readonly_segment);
    }

    if command.contains(';') {
        return command
            .split(';')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .all(is_safe_readonly_segment);
    }

    is_safe_readonly_segment(command)
}

fn is_safe_readonly_segment(segment: &str) -> bool {
    let normalized = segment.trim();
    if normalized.is_empty() {
        return false;
    }

    if contains_write_operator(normalized) || contains_network_indicator(normalized) {
        return false;
    }

    let Some(program) = normalized.split_whitespace().next() else {
        return false;
    };

    match program {
        "cd" | "set-location" | "pushd" => {
            let location = normalized
                .split_whitespace()
                .skip(1)
                .collect::<Vec<_>>()
                .join(" ");
            !location.is_empty()
                && !location.starts_with("..")
                && !location.contains("/../")
                && !location.contains("\\..\\")
        }
        "pwd" | "ls" | "dir" | "cat" | "type" | "head" | "tail" | "find" | "tree" | "rg" | "fd"
        | "findstr" | "select-string" | "get-childitem" | "get-content" | "measure-object"
        | "where-object" | "sort-object" | "select-object" => true,
        "git" => is_safe_git_command(normalized),
        _ => false,
    }
}

fn normalize_command(command: &str) -> String {
    command.trim().to_ascii_lowercase()
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

fn is_dangerous_command(command: &str) -> bool {
    let markers = [
        "rm -rf",
        "rm -fr",
        "remove-item -recurse",
        "del /s",
        "rmdir /s",
        "format ",
    ];
    markers.iter().any(|m| command.contains(m))
}

fn is_safe_git_command(command: &str) -> bool {
    let safe_git_prefixes = [
        "git status",
        "git diff",
        "git log",
        "git show",
        "git branch",
        "git rev-parse",
        "git ls-files",
        "git grep",
        "git cat-file",
    ];
    safe_git_prefixes
        .iter()
        .any(|prefix| command.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use serde_json::json;

    fn shell_call(command: &str) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: "shell_command".to_string(),
            arguments: json!({ "command": command }),
        }
    }

    fn shell_call_with_workdir(command: &str, workdir: &str) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: "shell_command".to_string(),
            arguments: json!({ "command": command, "workdir": workdir }),
        }
    }

    fn shell_spec() -> ToolSpec {
        ToolSpec {
            name: "shell_command".to_string(),
            description: String::new(),
            parameters: Value::Null,
            mutating: true,
            requires_approval: true,
            item_kind: agent_protocol::TurnItemKind::CommandExecution,
            delta_kind: agent_protocol::TurnItemDeltaKind::CommandExecutionOutput,
            approval_reason: Some(
                "Shell commands can inspect or modify the workspace.".to_string(),
            ),
        }
    }

    #[test]
    fn safe_readonly_shell_command_skips_approval() {
        let requirement = approval_requirement_for_tool(
            &shell_spec(),
            &shell_call("git log --oneline -10"),
            Path::new("D:\\learn\\gifti\\cloudagent"),
        );

        assert_eq!(requirement, ApprovalRequirement::not_required());
    }

    #[test]
    fn safe_readonly_shell_command_chain_skips_approval() {
        let requirement = approval_requirement_for_tool(
            &shell_spec(),
            &shell_call("cd D:\\learn\\gifti\\cloudagent && Get-ChildItem -Recurse -Filter *.rs"),
            Path::new("D:\\learn\\gifti\\cloudagent"),
        );

        assert_eq!(requirement, ApprovalRequirement::not_required());
    }

    #[test]
    fn rg_and_git_grep_skip_approval() {
        for command in [
            "rg -n approval_policy crates/agent-runtime/src/tools",
            "git grep approval_policy crates/agent-runtime/src/tools",
            "git ls-files crates/agent-tools/src",
        ] {
            let requirement = approval_requirement_for_tool(
                &shell_spec(),
                &shell_call(command),
                Path::new("D:\\learn\\gifti\\cloudagent"),
            );

            assert_eq!(requirement, ApprovalRequirement::not_required());
        }
    }

    #[test]
    fn shell_command_with_write_operator_requires_approval() {
        let requirement = approval_requirement_for_tool(
            &shell_spec(),
            &shell_call("echo hi > out.txt"),
            Path::new("D:\\learn\\gifti\\cloudagent"),
        );

        assert!(requirement.requires_approval);
    }

    #[test]
    fn shell_command_with_parent_workdir_requires_approval() {
        let requirement = approval_requirement_for_tool(
            &shell_spec(),
            &shell_call_with_workdir("git status", "..\\.."),
            Path::new("D:\\learn\\gifti\\cloudagent"),
        );

        assert!(requirement.requires_approval);
    }

    #[test]
    fn readonly_shell_command_can_read_outside_workspace_without_approval() {
        let requirement = approval_requirement_for_tool(
            &shell_spec(),
            &shell_call("type D:\\other\\notes.txt"),
            Path::new("D:\\learn\\gifti\\cloudagent"),
        );

        assert_eq!(requirement, ApprovalRequirement::not_required());
    }
}
