use agent_core::{
    ApprovalGrantKey, ApprovalPolicy, ApprovalRequirement, PermissionProfile, ToolCall, ToolSpec,
};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

pub fn approval_requirement_for_tool(
    spec: &ToolSpec,
    call: &ToolCall,
    workspace_root: &Path,
    permission_profile: &PermissionProfile,
    approval_policy: &ApprovalPolicy,
) -> ApprovalRequirement {
    match call.name.as_str() {
        "exec_command" => {
            exec_command_requirement(call, workspace_root, permission_profile, approval_policy)
        }
        _ if spec.requires_approval => match permission_profile {
            PermissionProfile::ReadOnly => ApprovalRequirement::required(
                spec.approval_reason
                    .clone()
                    .unwrap_or_else(|| format!("Tool `{}` requires approval.", call.name)),
            ),
            PermissionProfile::WorkspaceWrite | PermissionProfile::FullAccess => {
                match approval_policy {
                    ApprovalPolicy::Never => ApprovalRequirement::not_required(),
                    ApprovalPolicy::OnRequest => ApprovalRequirement::not_required(),
                }
            }
        },
        _ => ApprovalRequirement::not_required(),
    }
}

pub fn approval_grant_key_for_tool(
    spec: &ToolSpec,
    call: &ToolCall,
    _workspace_root: &Path,
    _permission_profile: &PermissionProfile,
    _approval_policy: &ApprovalPolicy,
) -> Option<ApprovalGrantKey> {
    match call.name.as_str() {
        "exec_command" => exec_command_grant_key(call),
        _ if spec.requires_approval => Some(ApprovalGrantKey::new(
            "tool_session",
            json!({
                "identity": call.identity,
            }),
        )),
        _ => None,
    }
}

#[derive(Deserialize)]
struct ExecCommandArgs {
    command: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    stdin: Option<String>,
    #[serde(default)]
    start_new_session: Option<bool>,
}

fn exec_command_requirement(
    call: &ToolCall,
    _workspace_root: &Path,
    permission_profile: &PermissionProfile,
    approval_policy: &ApprovalPolicy,
) -> ApprovalRequirement {
    let Ok(args) = serde_json::from_value::<ExecCommandArgs>(call.arguments.clone()) else {
        return ApprovalRequirement::required(
            "Command execution requires approval when its arguments cannot be classified safely.",
        );
    };

    let command = args.command.as_deref().unwrap_or("").trim();
    if args.session_id.is_some() && args.stdin.as_deref().is_some() {
        return ApprovalRequirement::required(
            "Interactive command session writes require approval.",
        );
    }
    if args.start_new_session.unwrap_or(false) {
        return ApprovalRequirement::required("Long-running command sessions require approval.");
    }
    if command.is_empty() {
        return ApprovalRequirement::required("Empty command executions require approval.");
    }

    let normalized = normalize_command(command);
    let readonly = is_safe_readonly_command(command);
    let workdir_escape = workdir_mentions_parent_escape(args.workdir.as_deref());
    let dangerous = is_dangerous_command(&normalized);
    let network = contains_network_indicator(&normalized);
    if dangerous {
        return ApprovalRequirement::required(
            "Dangerous commands (e.g. rm -rf / recursive delete) require approval.",
        );
    }

    match permission_profile {
        PermissionProfile::FullAccess => match approval_policy {
            ApprovalPolicy::Never => ApprovalRequirement::not_required(),
            ApprovalPolicy::OnRequest => {
                if readonly {
                    ApprovalRequirement::not_required()
                } else {
                    ApprovalRequirement::required(
                        "Mutating or network commands require approval under the current approval policy.",
                    )
                }
            }
        },
        PermissionProfile::WorkspaceWrite => {
            if workdir_escape {
                return ApprovalRequirement::required(
                    "This command needs permissions outside the workspace.",
                );
            }
            match approval_policy {
                ApprovalPolicy::Never => ApprovalRequirement::not_required(),
                ApprovalPolicy::OnRequest => {
                    if network {
                        ApprovalRequirement::required(
                            "Network commands require approval under the current approval policy.",
                        )
                    } else {
                        ApprovalRequirement::not_required()
                    }
                }
            }
        }
        PermissionProfile::ReadOnly => {
            if readonly && !workdir_escape {
                ApprovalRequirement::not_required()
            } else {
                ApprovalRequirement::required(
                    "Read-only permissions do not allow this command without explicit approval.",
                )
            }
        }
    }
}

fn exec_command_grant_key(call: &ToolCall) -> Option<ApprovalGrantKey> {
    let args = serde_json::from_value::<ExecCommandArgs>(call.arguments.clone()).ok()?;
    let command = args.command.as_deref().unwrap_or("").trim();
    if command.is_empty() {
        return None;
    }

    Some(ApprovalGrantKey::new(
        "exec_command",
        json!({
            "identity": call.identity,
            "command": normalize_command(command),
            "workdir": args.workdir.as_deref().map(str::trim).filter(|value| !value.is_empty()),
            "session_id_present": args.session_id.is_some(),
            "stdin_present": args.stdin.as_deref().is_some_and(|value| !value.is_empty()),
            "start_new_session": args.start_new_session.unwrap_or(false),
        }),
    ))
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
        "yarn add",
        "cargo install",
        "go get",
        "pip install",
    ];
    network_markers
        .iter()
        .any(|marker| command.contains(marker))
}

fn is_safe_git_command(command: &str) -> bool {
    [
        "git status",
        "git diff",
        "git show",
        "git log",
        "git branch",
        "git rev-parse",
        "git cat-file",
        "git ls-files",
        "git grep",
    ]
    .iter()
    .any(|prefix| command.starts_with(prefix))
}

fn is_dangerous_command(command: &str) -> bool {
    let dangerous_markers = [
        "rm -rf /",
        "rm -rf *",
        "del /s",
        "format ",
        "mkfs",
        "diskpart",
        "shutdown ",
        "reboot ",
        "init 0",
    ];
    dangerous_markers
        .iter()
        .any(|marker| command.contains(marker))
}
