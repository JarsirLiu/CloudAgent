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

pub struct ReadDirectoryTool;

impl ReadDirectoryTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            vec!["explore", "fs", "general"],
            ToolSpec {
                name: "read_directory".to_string(),
                description: "List direct children of a directory. Use this sparingly for structure confirmation, not as the primary repository discovery method.".to_string(),
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
struct ReadDirectoryArgs {
    #[serde(default)]
    path: Option<String>,
}

pub(crate) struct ReadDirectoryLocalTool;

#[async_trait]
impl LocalTool for ReadDirectoryLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadDirectoryTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadDirectoryArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, args.path.as_deref())?;
        let mut entries = fs::read_dir(&path).await?;
        let mut items = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            items.push(json!({
                "name": entry.file_name().to_string_lossy().to_string(),
                "path": entry.path().display().to_string(),
                "kind": if metadata.is_dir() { "dir" } else { "file" },
                "size": metadata.len(),
            }));
        }
        items.sort_by(|l, r| l["name"].as_str().unwrap_or_default().cmp(r["name"].as_str().unwrap_or_default()));
        Ok(ToolInvocationOutput {
            content: serde_json::to_string_pretty(&items)?,
            structured: Some(agent_protocol::StructuredToolResult::ListDirectory {
                path: path.display().to_string(),
                entry_count: items.len(),
            }),
        })
    }
}
