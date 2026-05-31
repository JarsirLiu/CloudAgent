use crate::command_access::{CommandAccess, classify_command};
use crate::impls::command::descriptor::ExecCommandTool;
use crate::impls::command::output::{CommandResultView, format_exec_result_content};
use crate::impls::command::session::ExecSessionStore;
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_external_read_path,
    resolve_write_path,
};
use agent_core::{
    CommandExecutionStatus, PermissionProfile, StructuredToolResult, ToolExecutionContext, ToolSpec,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
struct ExecCommandArgs {
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

pub(crate) struct ExecCommandLocalTool {
    sessions: Arc<ExecSessionStore>,
}

impl ExecCommandLocalTool {
    pub(crate) fn shared_pair() -> (
        Self,
        crate::impls::command::write_stdin::WriteStdinLocalTool,
    ) {
        let sessions = Arc::new(ExecSessionStore::new());
        (
            Self {
                sessions: Arc::clone(&sessions),
            },
            crate::impls::command::write_stdin::WriteStdinLocalTool::new(sessions),
        )
    }
}

#[async_trait]
impl LocalTool for ExecCommandLocalTool {
    fn spec(&self) -> ToolSpec {
        ExecCommandTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ExecCommandArgs = invocation.payload.parse_arguments()?;
        let timeout_ms = args
            .timeout_ms
            .unwrap_or(ctx.default_shell_timeout_ms)
            .max(1_000);
        let command = args
            .command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("`command` is required"))?;
        let command_access = classify_command(command);
        let workdir = resolve_command_workdir(
            &ctx.workspace_root,
            &ctx.permission_profile,
            command_access,
            args.workdir.as_deref(),
        )?;

        if let Some(rejection) =
            reject_workspace_file_edit_command(command, command_access, &workdir)
        {
            return Ok(rejection);
        }

        self.sessions
            .start_session(
                &ctx.conversation_id,
                command,
                workdir,
                matches!(ctx.permission_profile, PermissionProfile::FullAccess),
                timeout_ms,
                ctx,
            )
            .await
    }
}

fn resolve_command_workdir(
    workspace_root: &std::path::Path,
    permission_profile: &PermissionProfile,
    command_access: CommandAccess,
    workdir: Option<&str>,
) -> Result<std::path::PathBuf> {
    if matches!(
        permission_profile,
        PermissionProfile::WorkspaceWrite | PermissionProfile::FullAccess
    ) && command_access.is_read_only()
    {
        return resolve_external_read_path(workspace_root, workdir);
    }

    resolve_write_path(workspace_root, permission_profile, workdir)
}

fn reject_workspace_file_edit_command(
    command: &str,
    command_access: CommandAccess,
    workdir: &std::path::Path,
) -> Option<ToolInvocationOutput> {
    let reason = if looks_like_apply_patch_command(command) {
        Some("patch")
    } else if command_access.is_direct_file_write() {
        Some("direct file write")
    } else {
        None
    }?;

    Some(reject_file_edit_via_exec_command(command, workdir, reason))
}

fn looks_like_apply_patch_command(command: &str) -> bool {
    let normalized = command.trim().to_ascii_lowercase();
    normalized.starts_with("apply_patch ")
        || normalized == "apply_patch"
        || normalized.contains("\napply_patch ")
        || normalized.contains("&& apply_patch ")
        || normalized.contains("; apply_patch ")
}

fn reject_file_edit_via_exec_command(
    command: &str,
    workdir: &std::path::Path,
    reason: &str,
) -> ToolInvocationOutput {
    let current_directory = workdir.display().to_string();
    let message = format!(
        "Use the dedicated file editing tool instead of exec_command for workspace file edits ({reason}). Prefer `apply_patch` for structured workspace file changes."
    );
    let content = format_exec_result_content(CommandResultView {
        kind: "edit",
        command,
        current_directory: &current_directory,
        session_id: None,
        status: CommandExecutionStatus::Failed,
        exit_code: None,
        success: Some(false),
        stdout: "",
        stderr: &message,
    });
    ToolInvocationOutput {
        content: content.clone(),
        structured: Some(StructuredToolResult::CommandExecution {
            command: command.to_string(),
            current_directory,
            session_id: None,
            status: CommandExecutionStatus::Failed,
            exit_code: None,
            success: Some(false),
            stdout: Some(String::new()),
            stderr: Some(message),
            aggregated_output: Some(content),
            duration_ms: Some(0),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{
        LocalTool, LocalToolInvocation, LocalToolPayload, LocalToolSource,
    };
    use agent_core::ToolExecutionContext;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn exec_command_schema_does_not_accept_session_controls() {
        let parameters = ExecCommandTool::descriptor().spec.parameters;
        let properties = parameters
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("schema properties");

        assert!(properties.contains_key("command"));
        assert!(properties.contains_key("workdir"));
        assert!(properties.contains_key("timeout_ms"));
        assert!(!properties.contains_key("session_id"));
        assert!(!properties.contains_key("stdin"));
        assert!(!properties.contains_key("start_new_session"));
        assert_eq!(
            parameters.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
    }

    #[test]
    fn command_access_summary_classifies_common_commands() {
        assert_eq!(
            classify_command("rg -n TODO src").summary("rg -n TODO src"),
            "search"
        );
        assert_eq!(
            classify_command("git ls-files crates").summary("git ls-files crates"),
            "list files"
        );
        assert_eq!(
            classify_command("git status").summary("git status"),
            "inspect"
        );
        assert_eq!(
            classify_command("Set-Content out.txt hi").summary("Set-Content out.txt hi"),
            "action"
        );
    }

    #[test]
    fn command_workdir_allows_external_read_for_workspace_write() {
        let base = std::env::temp_dir().join(format!(
            "agent-tools-exec-perms-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = base.join("workspace");
        let outside = base.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let resolved = resolve_command_workdir(
            &workspace,
            &PermissionProfile::WorkspaceWrite,
            classify_command("Get-ChildItem -Force"),
            Some(&outside.to_string_lossy()),
        )
        .expect("workspace write can read external workdir");

        assert_eq!(
            resolved.canonicalize().unwrap(),
            outside.canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn command_workdir_rejects_external_write_for_workspace_write() {
        let base = std::env::temp_dir().join(format!(
            "agent-tools-exec-perms-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = base.join("workspace");
        let outside = base.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let err = resolve_command_workdir(
            &workspace,
            &PermissionProfile::WorkspaceWrite,
            classify_command("Set-Content out.txt hi"),
            Some(&outside.to_string_lossy()),
        )
        .expect_err("workspace write cannot write external workdir");

        assert!(err.to_string().contains("path escapes the workspace root"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[tokio::test]
    async fn exec_command_rejects_apply_patch_style_commands() {
        let (tool, _) = ExecCommandLocalTool::shared_pair();
        let ctx = test_context(PermissionProfile::ReadOnly);

        let output = tool
            .invoke(
                command_invocation(
                    "apply_patch *** Begin Patch\n*** Update File: src/lib.rs\n*** End Patch",
                ),
                &ctx,
            )
            .await
            .expect("exec command handled");

        match output.structured {
            Some(StructuredToolResult::CommandExecution { status, stderr, .. }) => {
                assert_eq!(status, CommandExecutionStatus::Failed);
                assert!(
                    stderr
                        .unwrap_or_default()
                        .contains("Use the dedicated file editing tool instead")
                );
            }
            other => panic!("expected structured command rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn exec_command_rejects_direct_file_write_commands() {
        let (tool, _) = ExecCommandLocalTool::shared_pair();
        let ctx = test_context(PermissionProfile::WorkspaceWrite);

        let output = tool
            .invoke(
                command_invocation(
                    "$text = Get-Content cli/src/ui/widgets/footer.rs -Raw; Set-Content cli/src/ui/widgets/footer.rs $text",
                ),
                &ctx,
            )
            .await
            .expect("exec command handled");

        match output.structured {
            Some(StructuredToolResult::CommandExecution { status, stderr, .. }) => {
                assert_eq!(status, CommandExecutionStatus::Failed);
                let stderr = stderr.unwrap_or_default();
                assert!(stderr.contains("direct file write"));
                assert!(stderr.contains("dedicated file editing tool"));
            }
            other => panic!("expected structured command rejection, got {other:?}"),
        }
    }

    fn test_context(permission_profile: PermissionProfile) -> ToolExecutionContext {
        ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: std::env::temp_dir(),
            conversation_store_dir: std::env::temp_dir(),
            permission_profile,
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn command_invocation(command: &str) -> LocalToolInvocation {
        LocalToolInvocation {
            identity: agent_core::ToolIdentity::built_in("exec_command"),
            source: LocalToolSource::BuiltIn,
            payload: LocalToolPayload::Function {
                arguments: serde_json::json!({ "command": command }),
            },
        }
    }
}
