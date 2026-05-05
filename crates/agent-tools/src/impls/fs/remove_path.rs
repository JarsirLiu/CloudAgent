use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_write_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolLayer, ToolPermissionTier, ToolRisk,
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

pub struct RemovePathTool;

impl RemovePathTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::High,
            ToolPermissionTier::WorkspaceWrite,
            vec!["edit", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 1,
                preferred_for: vec![
                    "removing one known generated file or directory",
                    "cleaning up an artifact path after verification",
                ],
                avoid_for: vec!["source discovery", "routine source editing"],
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "remove_path".to_string(),
                identity: ToolIdentity::built_in("remove_path"),
                description:
                    "Remove one known file or directory inside the workspace. This is a destructive filesystem primitive."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "recursive": { "type": "boolean" },
                        "force": { "type": "boolean" }
                    },
                    "required": ["path"]
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_layer(ToolLayer::PlatformFs)
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Debug, Deserialize)]
struct RemovePathArgs {
    path: String,
    #[serde(default)]
    recursive: Option<bool>,
    #[serde(default)]
    force: Option<bool>,
}

pub(crate) struct RemovePathLocalTool;

#[async_trait]
impl LocalTool for RemovePathLocalTool {
    fn spec(&self) -> ToolSpec {
        RemovePathTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: RemovePathArgs = invocation.payload.parse_arguments()?;
        let path = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.path.as_str()),
        )?;
        let recursive = args.recursive.unwrap_or(true);
        let force = args.force.unwrap_or(true);

        let metadata = match fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && force => {
                return Ok(ToolInvocationOutput {
                    content: format!("Path `{}` was already absent.", path.display()),
                    structured: Some(agent_protocol::StructuredToolResult::RemovePath {
                        path: path.display().to_string(),
                        recursive,
                        force,
                        removed: false,
                        status: WriteFileStatus::Completed,
                    }),
                });
            }
            Err(err) => return Err(err.into()),
        };

        if metadata.is_file() || metadata.file_type().is_symlink() {
            fs::remove_file(&path).await?;
        } else if metadata.is_dir() {
            if recursive {
                fs::remove_dir_all(&path).await?;
            } else {
                fs::remove_dir(&path).await?;
            }
        } else {
            bail!("remove_path only supports regular files, symlinks, and directories");
        }

        Ok(ToolInvocationOutput {
            content: format!("Removed `{}`.", path.display()),
            structured: Some(agent_protocol::StructuredToolResult::RemovePath {
                path: path.display().to_string(),
                recursive,
                force,
                removed: true,
                status: WriteFileStatus::Completed,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{LocalToolPayload, LocalToolSource};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn remove_path_removes_file() {
        let base = test_workspace("remove_path_removes_file");
        fs::create_dir_all(base.join("out")).await.expect("mkdir");
        fs::write(base.join("out/file.txt"), "demo\n")
            .await
            .expect("write");

        let tool = RemovePathLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("remove_path"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "path": "out/file.txt"
                        }),
                    },
                },
                &tool_context(&base),
            )
            .await
            .expect("remove_path works");

        assert!(!base.join("out/file.txt").exists());
        assert!(matches!(
            output.structured,
            Some(agent_protocol::StructuredToolResult::RemovePath { removed, .. }) if removed
        ));
    }

    #[tokio::test]
    async fn remove_path_force_allows_missing_path() {
        let base = test_workspace("remove_path_force_allows_missing_path");
        let tool = RemovePathLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("remove_path"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "path": "missing/file.txt",
                            "force": true
                        }),
                    },
                },
                &tool_context(&base),
            )
            .await
            .expect("remove_path works");

        assert!(matches!(
            output.structured,
            Some(agent_protocol::StructuredToolResult::RemovePath { removed, .. }) if !removed
        ));
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
