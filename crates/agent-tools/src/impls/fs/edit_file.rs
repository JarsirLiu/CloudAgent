use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::ToolSpec;
use agent_core::ToolExecutionContext;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeSet;
use tokio::fs;

pub struct EditFileTool;

impl EditFileTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Medium,
            vec!["edit", "fs", "general"],
            ToolSpec {
            name: "edit_file".to_string(),
                description: "Apply a focused patch to existing files. Prefer this over whole-file rewrites for code changes.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "patch": { "type": "string" }
                    },
                    "required": ["patch"]
                }),
                mutating: true,
                requires_approval: true,
                item_kind: agent_protocol::TurnItemKind::FileChange,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: Some("Applying patches can modify workspace files.".to_string()),
            },
        )
    }
}

#[derive(Deserialize)]
struct EditFileArgs {
    patch: String,
}

pub(crate) struct EditFileLocalTool;

#[async_trait]
impl LocalTool for EditFileLocalTool {
    fn spec(&self) -> ToolSpec {
        EditFileTool::descriptor().spec
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
