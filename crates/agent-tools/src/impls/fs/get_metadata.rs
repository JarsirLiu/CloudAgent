use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::time::UNIX_EPOCH;
use tokio::fs;

pub struct GetMetadataTool;

impl GetMetadataTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["explore", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 2,
                preferred_for: vec![
                    "checking whether one exact path exists",
                    "verifying file type or size after a path is already known",
                ],
                avoid_for: vec![
                    "repository discovery",
                    "choosing which source file to read next",
                ],
                follow_up_hint: Some(
                    "use `search_workspace` to discover candidate files and `read_file` to inspect source",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "get_metadata".to_string(),
                identity: ToolIdentity::built_in("get_metadata"),
                description:
                    "Read focused path metadata such as existence, file type, size, and readonly status."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::ParallelSafe,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Deserialize)]
struct GetMetadataArgs {
    path: String,
}

pub(crate) struct GetMetadataLocalTool;

#[async_trait]
impl LocalTool for GetMetadataLocalTool {
    fn spec(&self) -> ToolSpec {
        GetMetadataTool::descriptor().spec
    }
    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: GetMetadataArgs = invocation.payload.parse_arguments()?;
        let path = resolve_read_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let metadata = fs::symlink_metadata(&path).await?;
        let created_at_ms = metadata
            .created()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| u64::try_from(duration.as_millis()).ok());
        let modified_at_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| u64::try_from(duration.as_millis()).ok());
        let value = json!({
            "path": path.display().to_string(),
            "exists": true,
            "is_file": metadata.is_file(),
            "is_dir": metadata.is_dir(),
            "is_symlink": metadata.file_type().is_symlink(),
            "size": metadata.len(),
            "readonly": metadata.permissions().readonly(),
            "created_at_ms": created_at_ms,
            "modified_at_ms": modified_at_ms
        });
        Ok(ToolInvocationOutput {
            content: serde_json::to_string_pretty(&value)?,
            structured: Some(agent_protocol::StructuredToolResult::GetMetadata {
                path: path.display().to_string(),
                exists: true,
                is_file: metadata.is_file(),
                is_dir: metadata.is_dir(),
                is_symlink: metadata.file_type().is_symlink(),
                size: metadata.len(),
                readonly: metadata.permissions().readonly(),
                created_at_ms,
                modified_at_ms,
            }),
        })
    }
}
