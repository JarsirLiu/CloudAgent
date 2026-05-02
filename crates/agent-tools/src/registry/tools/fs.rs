use crate::impls::fs::{
    EditFileTool as EditFileDescriptorTool, GetMetadataTool,
    ReadDirectoryTool as ReadDirectoryDescriptorTool,
    WriteFileTool as WriteFileDescriptorTool,
};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::{ToolExecutionContext, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tokio::fs;

pub(crate) struct GetMetadataLocalTool;
pub(crate) struct ReadDirectoryTool;
pub(crate) struct WriteFileTool;
pub(crate) struct EditFileLocalTool;

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
struct EditFileArgs {
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
            structured: Some(agent_protocol::StructuredToolResult::WriteFile {
                path: path.display().to_string(),
                bytes_written,
                status: agent_protocol::WriteFileStatus::Completed,
            }),
        })
    }
}

#[async_trait]
impl LocalTool for EditFileLocalTool {
    fn spec(&self) -> ToolSpec {
        EditFileDescriptorTool::descriptor().spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: EditFileArgs = serde_json::from_value(arguments)?;
        let file_patches = parse_unified_patch(&args.patch)?;
        if file_patches.is_empty() {
            anyhow::bail!("patch did not contain any editable file hunks");
        }
        let mut changed_files = BTreeSet::new();
        for file_patch in file_patches {
            if file_patch.path == "/dev/null" {
                anyhow::bail!("creating new files through edit_file is not supported");
            }
            let path = resolve_workspace_path(&ctx.workspace_root, Some(file_patch.path.as_str()))?;
            let current = fs::read_to_string(&path).await?;
            let next = apply_hunks(&current, &file_patch.hunks)
                .map_err(|err| anyhow::anyhow!("failed to apply patch for {}: {err}", file_patch.path))?;
            if next != current {
                let Some(parent) = path.parent() else {
                    anyhow::bail!("cannot determine parent directory for {}", path.display());
                };
                fs::create_dir_all(parent).await?;
                fs::write(&path, next).await?;
                changed_files.insert(file_patch.path);
            }
        }
        let files_changed = changed_files.len();

        Ok(ToolInvocationOutput {
            content: format!("Applied patch. files_changed={files_changed}"),
            structured: Some(agent_protocol::StructuredToolResult::ApplyPatch {
                files_changed,
                status: agent_protocol::WriteFileStatus::Completed,
            }),
        })
    }
}

#[derive(Debug)]
struct FilePatch {
    path: String,
    hunks: Vec<Hunk>,
}

#[derive(Debug)]
struct Hunk {
    lines: Vec<String>,
}

fn parse_unified_patch(patch: &str) -> anyhow::Result<Vec<FilePatch>> {
    let mut file_patches = Vec::new();
    let mut current_path: Option<String> = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_hunk: Option<Hunk> = None;

    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            if let Some(path) = current_path.take() {
                file_patches.push(FilePatch { path, hunks });
                hunks = Vec::new();
            }
            let normalized = path
                .trim()
                .strip_prefix("b/")
                .unwrap_or(path.trim())
                .to_string();
            current_path = Some(normalized);
            continue;
        }
        if line.starts_with("@@") {
            if current_path.is_none() {
                anyhow::bail!("hunk found before target file");
            }
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            current_hunk = Some(Hunk { lines: Vec::new() });
            continue;
        }
        if let Some(hunk) = current_hunk.as_mut() {
            if line.starts_with('\\') {
                continue;
            }
            hunk.lines.push(line.to_string());
        }
    }
    if let Some(h) = current_hunk.take() {
        hunks.push(h);
    }
    if let Some(path) = current_path.take() {
        file_patches.push(FilePatch { path, hunks });
    }
    Ok(file_patches)
}

fn apply_hunks(original: &str, hunks: &[Hunk]) -> anyhow::Result<String> {
    let mut content = original.to_string();
    for hunk in hunks {
        content = apply_single_hunk(&content, hunk)?;
    }
    Ok(content)
}

fn apply_single_hunk(content: &str, hunk: &Hunk) -> anyhow::Result<String> {
    let mut old_block: Vec<String> = Vec::new();
    let mut new_block: Vec<String> = Vec::new();
    for line in &hunk.lines {
        if let Some(rest) = line.strip_prefix(' ') {
            old_block.push(rest.to_string());
            new_block.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('-') {
            old_block.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix('+') {
            new_block.push(rest.to_string());
        } else {
            anyhow::bail!("unsupported hunk line: {line}");
        }
    }

    let old_text = old_block.join("\n");
    let replacement = new_block.join("\n");
    if old_text.is_empty() {
        anyhow::bail!("empty deletion context is not supported");
    }
    let Some(start) = content.find(&old_text) else {
        anyhow::bail!("could not find hunk context in target file");
    };
    let end = start + old_text.len();
    let mut next = String::with_capacity(content.len() - old_text.len() + replacement.len());
    next.push_str(&content[..start]);
    next.push_str(&replacement);
    next.push_str(&content[end..]);
    Ok(next)
}
