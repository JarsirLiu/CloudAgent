use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_read_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolExecutionContext;
use agent_core::ToolSpec;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::fs;

pub struct FsStatTool;

impl FsStatTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            vec!["explore", "verify", "general"],
            ToolSpec {
                name: "fs_stat".to_string(),
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
struct FsStatArgs {
    path: String,
}

pub(crate) struct FsStatLocalTool;

#[async_trait]
impl LocalTool for FsStatLocalTool {
    fn spec(&self) -> ToolSpec {
        FsStatTool::descriptor().spec
    }
    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: FsStatArgs = serde_json::from_value(arguments)?;
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
