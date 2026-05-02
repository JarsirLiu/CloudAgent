use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::ToolSpec;
use agent_core::ToolExecutionContext;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::fs;

pub struct WriteFileTool;

impl WriteFileTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::High,
            vec!["edit", "fs", "general"],
            ToolSpec {
                name: "write_file".to_string(),
                description: "Create or replace a file when patch-based editing is not appropriate.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" },
                        "overwrite": { "type": "boolean" }
                    },
                    "required": ["path", "content"]
                }),
                mutating: true,
                requires_approval: true,
                item_kind: agent_protocol::TurnItemKind::FileChange,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: Some("Writing files can modify workspace contents.".to_string()),
            },
        )
    }
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
    #[serde(default)]
    overwrite: Option<bool>,
}

pub(crate) struct WriteFileLocalTool;

#[async_trait]
impl LocalTool for WriteFileLocalTool {
    fn spec(&self) -> ToolSpec {
        WriteFileTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: WriteFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        if path.exists() && !args.overwrite.unwrap_or(true) {
            anyhow::bail!("target file already exists and overwrite=false: {}", path.display());
        }
        let Some(parent) = path.parent() else {
            anyhow::bail!("cannot determine parent directory for {}", path.display());
        };
        fs::create_dir_all(parent).await?;
        let bytes_written = args.content.len();
        fs::write(&path, args.content).await?;
        Ok(ToolInvocationOutput {
            content: format!("Wrote {}", path.display()),
            structured: Some(agent_protocol::StructuredToolResult::WriteFile {
                path: path.display().to_string(),
                bytes_written,
                status: agent_protocol::WriteFileStatus::Completed,
            }),
        })
    }
}
