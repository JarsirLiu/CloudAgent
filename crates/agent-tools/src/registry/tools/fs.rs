use crate::impls::fs::{GetMetadataTool, ReadDirectoryTool as ReadDirectoryDescriptorTool};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::{ToolExecutionContext, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::fs;

pub(crate) struct GetMetadataLocalTool;
pub(crate) struct ReadDirectoryTool;

#[derive(Deserialize)]
struct GetMetadataArgs {
    path: String,
}

#[derive(Deserialize)]
struct ReadDirectoryArgs {
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl LocalTool for GetMetadataLocalTool {
    fn spec(&self) -> ToolSpec {
        GetMetadataTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: GetMetadataArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
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
            summary: format!("metadata for {}", path.display()),
            content: serde_json::to_string_pretty(&value)?,
            structured: None,
        })
    }
}

#[async_trait]
impl LocalTool for ReadDirectoryTool {
    fn spec(&self) -> ToolSpec {
        ReadDirectoryDescriptorTool::descriptor().spec
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
            summary: format!("listed {} entries", items.len()),
            structured: Some(agent_protocol::StructuredToolResult::ListDirectory {
                path: path.display().to_string(),
                entry_count: items.len(),
            }),
        })
    }
}
