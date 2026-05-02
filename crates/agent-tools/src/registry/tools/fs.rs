use crate::impls::fs::{
    ApplyPatchTool as ApplyPatchDescriptorTool, GetMetadataTool,
    ReadDirectoryTool as ReadDirectoryDescriptorTool,
    WriteFileTool as WriteFileDescriptorTool,
};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::{ToolExecutionContext, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::process::Command;

pub(crate) struct GetMetadataLocalTool;
pub(crate) struct ReadDirectoryTool;
pub(crate) struct WriteFileTool;
pub(crate) struct ApplyPatchLocalTool;

#[derive(Deserialize)]
struct GetMetadataArgs {
    path: String,
}

#[derive(Deserialize)]
struct ReadDirectoryArgs {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct ApplyPatchArgs {
    patch: String,
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

#[async_trait]
impl LocalTool for WriteFileTool {
    fn spec(&self) -> ToolSpec {
        WriteFileDescriptorTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: WriteFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let Some(parent) = path.parent() else {
            anyhow::bail!("cannot determine parent directory for {}", path.display());
        };
        fs::create_dir_all(parent).await?;
        let bytes_written = args.content.len();
        fs::write(&path, args.content).await?;
        Ok(ToolInvocationOutput {
            content: format!("Wrote {}", path.display()),
            summary: format!("wrote {}", path.display()),
            structured: Some(agent_protocol::StructuredToolResult::WriteFile {
                path: path.display().to_string(),
                bytes_written,
                status: agent_protocol::WriteFileStatus::Completed,
            }),
        })
    }
}

#[async_trait]
impl LocalTool for ApplyPatchLocalTool {
    fn spec(&self) -> ToolSpec {
        ApplyPatchDescriptorTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ApplyPatchArgs = serde_json::from_value(arguments)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let patch_path = ctx.workspace_root.join(format!(".apply_patch_{stamp}.diff"));
        fs::write(&patch_path, &args.patch).await?;

        let files_changed = count_patch_targets(&args.patch);
        let output = Command::new("git")
            .current_dir(&ctx.workspace_root)
            .arg("apply")
            .arg("--whitespace=nowarn")
            .arg("--recount")
            .arg(&patch_path)
            .output()
            .await?;
        let _ = fs::remove_file(&patch_path).await;

        if !output.status.success() {
            anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim().to_string());
        }

        Ok(ToolInvocationOutput {
            content: format!("Applied patch. files_changed={files_changed}"),
            summary: "applied patch".to_string(),
            structured: Some(agent_protocol::StructuredToolResult::ApplyPatch {
                files_changed,
                status: agent_protocol::WriteFileStatus::Completed,
            }),
        })
    }
}

fn count_patch_targets(patch: &str) -> usize {
    let mut files = std::collections::BTreeSet::new();
    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            files.insert(path.to_string());
        }
    }
    files.len()
}
