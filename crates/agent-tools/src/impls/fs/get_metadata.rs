use crate::registry::shared::{LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk};
use agent_core::{ToolExecutionContext, ToolIdentity, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

pub struct GetMetadataTool;

impl GetMetadataTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            true,
            vec!["explore", "verify", "repo", "fs"],
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
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
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
        let metadata = fs::metadata(&path).await?;
        let value = json!({
            "path": path.display().to_string(),
            "exists": true,
            "is_file": metadata.is_file(),
            "is_dir": metadata.is_dir(),
            "size": metadata.len(),
            "readonly": metadata.permissions().readonly()
        });
        Ok(ToolInvocationOutput {
            content: serde_json::to_string_pretty(&value)?,
            structured: Some(agent_protocol::StructuredToolResult::GetMetadata {
                path: path.display().to_string(),
                exists: true,
                is_file: metadata.is_file(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                readonly: metadata.permissions().readonly(),
            }),
        })
    }
}
