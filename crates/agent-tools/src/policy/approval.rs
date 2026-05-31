use crate::command_access::classify_command;
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
        "write_stdin" => write_stdin_requirement(call, permission_profile, approval_policy),
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
}

#[derive(Deserialize)]
struct WriteStdinArgs {
    #[serde(default)]
    chars: Option<String>,
}

fn exec_command_requirement(
    call: &ToolCall,
    workspace_root: &Path,
    permission_profile: &PermissionProfile,
    approval_policy: &ApprovalPolicy,
) -> ApprovalRequirement {
    let Ok(args) = serde_json::from_value::<ExecCommandArgs>(call.arguments.clone()) else {
        return ApprovalRequirement::required(
            "Command execution requires approval when its arguments cannot be classified safely.",
        );
    };

    let command = args.command.as_deref().unwrap_or("").trim();
    if command.is_empty() {
        return ApprovalRequirement::required("Empty command executions require approval.");
    }

    let access = classify_command(command);
    let workdir_escape = workdir_escapes_workspace(workspace_root, args.workdir.as_deref());
    if access.is_dangerous() {
        return ApprovalRequirement::required(
            "Dangerous commands (e.g. rm -rf / recursive delete) require approval.",
        );
    }

    match permission_profile {
        PermissionProfile::FullAccess => match approval_policy {
            ApprovalPolicy::Never => ApprovalRequirement::not_required(),
            ApprovalPolicy::OnRequest => {
                if access.is_read_only() {
                    ApprovalRequirement::not_required()
                } else {
                    ApprovalRequirement::required(
                        "Mutating or network commands require approval under the current approval policy.",
                    )
                }
            }
        },
        PermissionProfile::WorkspaceWrite => {
            if !access.is_read_only() && workdir_escape {
                return ApprovalRequirement::required(
                    "Writing outside the workspace requires stronger permissions.",
                );
            }
            match approval_policy {
                ApprovalPolicy::Never => ApprovalRequirement::not_required(),
                ApprovalPolicy::OnRequest => {
                    if access.is_network() {
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
            if access.is_read_only() && !workdir_escape {
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
            "command": command.trim().to_ascii_lowercase(),
            "workdir": args.workdir.as_deref().map(str::trim).filter(|value| !value.is_empty()),
        }),
    ))
}

fn write_stdin_requirement(
    call: &ToolCall,
    permission_profile: &PermissionProfile,
    approval_policy: &ApprovalPolicy,
) -> ApprovalRequirement {
    let Ok(args) = serde_json::from_value::<WriteStdinArgs>(call.arguments.clone()) else {
        return ApprovalRequirement::required(
            "Interactive command input requires approval when its arguments cannot be classified safely.",
        );
    };
    let writes_input = args.chars.as_deref().is_some_and(|value| !value.is_empty());
    if !writes_input {
        return ApprovalRequirement::not_required();
    }

    match permission_profile {
        PermissionProfile::ReadOnly | PermissionProfile::WorkspaceWrite => {
            ApprovalRequirement::required(
                "Interactive command input requires stronger permissions because it can modify files.",
            )
        }
        PermissionProfile::FullAccess => match approval_policy {
            ApprovalPolicy::Never => ApprovalRequirement::not_required(),
            ApprovalPolicy::OnRequest => ApprovalRequirement::required(
                "Interactive command input requires approval under the current approval policy.",
            ),
        },
    }
}

fn workdir_escapes_workspace(workspace_root: &Path, workdir: Option<&str>) -> bool {
    let Some(workdir) = workdir.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let input = Path::new(workdir);
    let candidate = if input.is_absolute() {
        input.to_path_buf()
    } else {
        root.join(input)
    };
    let candidate = normalize_existing_ancestor_path(&candidate);
    !candidate.starts_with(&root)
}

fn normalize_existing_ancestor_path(path: &Path) -> std::path::PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    let mut suffix = Vec::new();
    let mut current = path;
    loop {
        let Some(name) = current.file_name() else {
            return path.to_path_buf();
        };
        suffix.push(name.to_os_string());
        let Some(parent) = current.parent() else {
            return path.to_path_buf();
        };
        if let Ok(canonical_parent) = parent.canonicalize() {
            let mut normalized = canonical_parent;
            for segment in suffix.iter().rev() {
                normalized.push(segment);
            }
            return normalized;
        }
        current = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolIdentity;
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: name.to_string(),
            identity: ToolIdentity::built_in(name),
            arguments,
        }
    }

    fn exec_call(arguments: serde_json::Value) -> ToolCall {
        tool_call("exec_command", arguments)
    }

    fn approval_for(arguments: serde_json::Value) -> ApprovalRequirement {
        exec_command_requirement(
            &exec_call(arguments),
            Path::new("."),
            &PermissionProfile::WorkspaceWrite,
            &ApprovalPolicy::OnRequest,
        )
    }

    fn unique_workspace() -> (PathBuf, PathBuf, PathBuf) {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("agent-tools-policy-{suffix}"));
        let workspace = base.join("workspace");
        let outside = base.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        (base, workspace, outside)
    }

    #[test]
    fn workspace_write_allows_non_network_commands() {
        let requirement = approval_for(json!({
            "command": "Get-ChildItem -Force -LiteralPath .\\data",
            "workdir": "."
        }));

        assert!(!requirement.requires_approval);
    }

    #[test]
    fn workspace_write_allows_readonly_listing_with_legacy_session_fields() {
        let requirement = approval_for(json!({
            "command": "Get-ChildItem -Force -LiteralPath data | Format-List Name,FullName,Mode,Length,LastWriteTime",
            "close_stdin": true,
            "workdir": "."
        }));

        assert!(
            !requirement.requires_approval,
            "legacy session fields must not turn a read-only listing into an approval request"
        );
    }

    #[test]
    fn workspace_write_requires_approval_for_network_commands() {
        let requirement = approval_for(json!({
            "command": "curl https://example.com",
            "workdir": "."
        }));

        assert_eq!(
            requirement.reason.as_deref(),
            Some("Network commands require approval under the current approval policy.")
        );
    }

    #[test]
    fn workspace_write_allows_readonly_command_in_external_workdir() {
        let (base, workspace, outside) = unique_workspace();

        let requirement = exec_command_requirement(
            &exec_call(json!({
                "command": "Get-ChildItem -Force",
                "workdir": outside.to_string_lossy()
            })),
            &workspace,
            &PermissionProfile::WorkspaceWrite,
            &ApprovalPolicy::OnRequest,
        );

        assert!(!requirement.requires_approval);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn workspace_write_requires_approval_for_mutating_external_workdir() {
        let (base, workspace, outside) = unique_workspace();

        let requirement = exec_command_requirement(
            &exec_call(json!({
                "command": "Set-Content out.txt hi",
                "workdir": outside.to_string_lossy()
            })),
            &workspace,
            &PermissionProfile::WorkspaceWrite,
            &ApprovalPolicy::OnRequest,
        );

        assert_eq!(
            requirement.reason.as_deref(),
            Some("Writing outside the workspace requires stronger permissions.")
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn full_access_still_requires_approval_for_dangerous_commands() {
        let (base, workspace, outside) = unique_workspace();

        let requirement = exec_command_requirement(
            &exec_call(json!({
                "command": format!("Remove-Item -Recurse -Force {}", outside.to_string_lossy()),
                "workdir": workspace.to_string_lossy()
            })),
            &workspace,
            &PermissionProfile::FullAccess,
            &ApprovalPolicy::Never,
        );

        assert_eq!(
            requirement.reason.as_deref(),
            Some("Dangerous commands (e.g. rm -rf / recursive delete) require approval.")
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn write_stdin_poll_does_not_require_approval() {
        let requirement = write_stdin_requirement(
            &tool_call(
                "write_stdin",
                json!({"session_id": "exec:test:1", "chars": ""}),
            ),
            &PermissionProfile::WorkspaceWrite,
            &ApprovalPolicy::OnRequest,
        );

        assert!(!requirement.requires_approval);
    }

    #[test]
    fn write_stdin_input_requires_stronger_permissions_for_workspace_write() {
        let requirement = write_stdin_requirement(
            &tool_call(
                "write_stdin",
                json!({"session_id": "exec:test:1", "chars": "exit\n"}),
            ),
            &PermissionProfile::WorkspaceWrite,
            &ApprovalPolicy::OnRequest,
        );

        assert_eq!(
            requirement.reason.as_deref(),
            Some(
                "Interactive command input requires stronger permissions because it can modify files."
            )
        );
    }
}
