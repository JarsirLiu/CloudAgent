use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_write_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolLayer, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{
    StructuredToolResult, ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec,
    TurnItemDeltaKind, TurnItemKind,
};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

pub struct CreateDirectoryTool;

impl CreateDirectoryTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::WorkspaceWrite,
            vec!["edit", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 4,
                preferred_for: vec![
                    "preparing a known directory before writing files",
                    "creating output folders for generated artifacts",
                ],
                avoid_for: vec!["editing source files", "discovering repository structure"],
                follow_up_hint: Some(
                    "follow with `apply_patch` for source changes or `copy_path` when seeding a directory from an existing path",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "create_directory".to_string(),
                identity: ToolIdentity::built_in("create_directory"),
                description:
                    "Create one directory path inside the workspace. This is a filesystem primitive, not a source-editing tool."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "recursive": { "type": "boolean" }
                    },
                    "required": ["path"]
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: false,
                item_kind: TurnItemKind::ToolCall,
                delta_kind: TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_layer(ToolLayer::PlatformFs)
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Debug, Deserialize)]
struct CreateDirectoryArgs {
    path: String,
    #[serde(default)]
    recursive: Option<bool>,
}

pub(crate) struct CreateDirectoryLocalTool;

#[async_trait]
impl LocalTool for CreateDirectoryLocalTool {
    fn spec(&self) -> ToolSpec {
        CreateDirectoryTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: CreateDirectoryArgs = invocation.payload.parse_arguments()?;
        let path = resolve_write_path(
            &ctx.workspace_root,
            &ctx.permission_profile,
            Some(args.path.as_str()),
        )?;
        let recursive = args.recursive.unwrap_or(true);

        if recursive {
            fs::create_dir_all(&path).await?;
        } else {
            fs::create_dir(&path).await?;
        }

        Ok(ToolInvocationOutput {
            content: format!("Created directory `{}`.", path.display()),
            structured: Some(StructuredToolResult::CreateDirectory {
                path: path.display().to_string(),
                recursive,
                created: true,
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
    async fn create_directory_creates_nested_path() {
        let base = test_workspace("create_directory_creates_nested_path");
        let tool = CreateDirectoryLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("create_directory"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "path": "out/artifacts",
                            "recursive": true
                        }),
                    },
                },
                &tool_context(&base),
            )
            .await
            .expect("create_directory works");

        assert!(base.join("out/artifacts").is_dir());
        assert!(matches!(
            output.structured,
            Some(StructuredToolResult::CreateDirectory { created, .. }) if created
        ));
    }

    fn tool_context(workspace_root: &std::path::Path) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::WorkspaceWrite,
            default_shell_timeout_ms: 5_000,
            max_tool_output_tokens:
                agent_core::ToolExecutionContext::default_max_tool_output_tokens(),
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
