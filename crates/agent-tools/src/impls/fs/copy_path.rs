use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_write_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{
    ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec, WriteFileStatus,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

pub struct CopyPathTool;

impl CopyPathTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Medium,
            ToolPermissionTier::WorkspaceWrite,
            vec!["edit", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 2,
                preferred_for: vec![
                    "copying one known file or directory",
                    "duplicating generated artifacts into a target location",
                ],
                avoid_for: vec!["source discovery", "small targeted code edits"],
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "copy_path".to_string(),
                identity: ToolIdentity::built_in("copy_path"),
                description:
                    "Copy one known file or directory to a destination inside the workspace."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "source_path": { "type": "string" },
                        "destination_path": { "type": "string" },
                        "recursive": { "type": "boolean" }
                    },
                    "required": ["source_path", "destination_path"]
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Debug, Deserialize)]
struct CopyPathArgs {
    source_path: String,
    destination_path: String,
    #[serde(default)]
    recursive: Option<bool>,
}

pub(crate) struct CopyPathLocalTool;

#[async_trait]
impl LocalTool for CopyPathLocalTool {
    fn spec(&self) -> ToolSpec {
        CopyPathTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: CopyPathArgs = invocation.payload.parse_arguments()?;
        let source_path = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.source_path.as_str()),
        )?;
        let destination_path = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.destination_path.as_str()),
        )?;
        let recursive = args.recursive.unwrap_or(false);

        let metadata = fs::symlink_metadata(&source_path).await?;
        if metadata.is_file() {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&source_path, &destination_path).await?;
        } else if metadata.is_dir() {
            if !recursive {
                bail!("copy_path requires `recursive: true` when copying a directory");
            }
            copy_directory_recursive(&source_path, &destination_path).await?;
        } else {
            bail!("copy_path only supports regular files and directories");
        }

        Ok(ToolInvocationOutput {
            content: format!(
                "Copied `{}` to `{}`.",
                source_path.display(),
                destination_path.display()
            ),
            structured: Some(agent_protocol::StructuredToolResult::CopyPath {
                source_path: source_path.display().to_string(),
                destination_path: destination_path.display().to_string(),
                recursive,
                status: WriteFileStatus::Completed,
            }),
        })
    }
}

async fn copy_directory_recursive(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<()> {
    let mut stack = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        fs::create_dir_all(&to).await?;
        let mut entries = fs::read_dir(&from).await?;
        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();
            let destination_path = to.join(entry.file_name());
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push((entry_path, destination_path));
            } else if file_type.is_file() {
                if let Some(parent) = destination_path.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::copy(&entry_path, &destination_path).await?;
            } else {
                bail!("copy_path only supports directories containing regular files");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{LocalToolPayload, LocalToolSource};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn copy_path_copies_file() {
        let base = test_workspace("copy_path_copies_file");
        fs::create_dir_all(base.join("src")).await.expect("mkdir");
        fs::write(base.join("src/lib.rs"), "demo\n")
            .await
            .expect("write");

        let tool = CopyPathLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("copy_path"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "source_path": "src/lib.rs",
                            "destination_path": "backup/lib.rs"
                        }),
                    },
                },
                &tool_context(&base),
            )
            .await
            .expect("copy_path works");

        let copied = fs::read_to_string(base.join("backup/lib.rs"))
            .await
            .expect("read copied");
        assert_eq!(copied, "demo\n");
        assert!(matches!(
            output.structured,
            Some(agent_protocol::StructuredToolResult::CopyPath {
                status: WriteFileStatus::Completed,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn copy_path_copies_directory_recursively() {
        let base = test_workspace("copy_path_copies_directory_recursively");
        fs::create_dir_all(base.join("src/nested"))
            .await
            .expect("mkdir");
        fs::write(base.join("src/nested/lib.rs"), "demo\n")
            .await
            .expect("write");

        let tool = CopyPathLocalTool;
        tool.invoke(
            LocalToolInvocation {
                identity: ToolIdentity::built_in("copy_path"),
                source: LocalToolSource::BuiltIn,
                payload: LocalToolPayload::Function {
                    arguments: json!({
                        "source_path": "src",
                        "destination_path": "shadow",
                        "recursive": true
                    }),
                },
            },
            &tool_context(&base),
        )
        .await
        .expect("copy_path works");

        let copied = fs::read_to_string(base.join("shadow/nested/lib.rs"))
            .await
            .expect("read copied");
        assert_eq!(copied, "demo\n");
    }

    fn tool_context(workspace_root: &std::path::Path) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::WorkspaceWrite,
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn test_workspace(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis();
        path.push(format!("cloudagent_{name}_{stamp}"));
        std::fs::create_dir_all(&path).expect("create temp workspace");
        path
    }
}

