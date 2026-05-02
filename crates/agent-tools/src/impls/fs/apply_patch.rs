use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolExecutionContext;
use agent_core::ToolSpec;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeSet;
use tokio::fs;

pub struct ApplyPatchTool;

impl ApplyPatchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Medium,
            vec!["edit", "general"],
            ToolSpec {
                name: "apply_patch".to_string(),
                description: "Apply a focused unified patch to one or more workspace files. Prefer this over whole-file rewrites for code changes.".to_string(),
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
struct ApplyPatchArgs {
    patch: String,
}

pub(crate) struct ApplyPatchLocalTool;

#[async_trait]
impl LocalTool for ApplyPatchLocalTool {
    fn spec(&self) -> ToolSpec {
        ApplyPatchTool::descriptor().spec
    }
    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ApplyPatchArgs = serde_json::from_value(arguments)?;
        let file_patches = parse_unified_patch(&args.patch)?;
        if file_patches.is_empty() {
            anyhow::bail!("patch did not contain any editable file hunks");
        }
        let mut changed_files = BTreeSet::new();
        for file_patch in file_patches {
            match (file_patch.old_path.as_str(), file_patch.new_path.as_str()) {
                ("/dev/null", new_path) => {
                    let path = resolve_workspace_path(&ctx.workspace_root, Some(new_path))?;
                    let next = render_hunks_as_new_file(&file_patch.hunks)?;
                    let Some(parent) = path.parent() else {
                        anyhow::bail!("cannot determine parent directory for {}", path.display());
                    };
                    fs::create_dir_all(parent).await?;
                    fs::write(&path, next).await?;
                    changed_files.insert(new_path.to_string());
                }
                (old_path, "/dev/null") => {
                    let path = resolve_workspace_path(&ctx.workspace_root, Some(old_path))?;
                    if path.exists() {
                        fs::remove_file(&path).await?;
                        changed_files.insert(old_path.to_string());
                    }
                }
                (_, new_path) => {
                    let path = resolve_workspace_path(&ctx.workspace_root, Some(new_path))?;
                    let current = fs::read_to_string(&path).await?;
                    let next = apply_hunks(&current, &file_patch.hunks).map_err(|err| {
                        anyhow::anyhow!("failed to apply patch for {}: {err}", new_path)
                    })?;
                    if next != current {
                        let Some(parent) = path.parent() else {
                            anyhow::bail!(
                                "cannot determine parent directory for {}",
                                path.display()
                            );
                        };
                        fs::create_dir_all(parent).await?;
                        fs::write(&path, next).await?;
                        changed_files.insert(new_path.to_string());
                    }
                }
            }
        }
        let files_changed = changed_files.len();

        Ok(ToolInvocationOutput {
            content: format!("Applied patch. files_changed={files_changed}"),
            structured: Some(agent_protocol::StructuredToolResult::EditFile {
                files_changed,
                status: agent_protocol::WriteFileStatus::Completed,
            }),
        })
    }
}

#[derive(Debug)]
struct FilePatch {
    old_path: String,
    new_path: String,
    hunks: Vec<Hunk>,
}

#[derive(Debug)]
struct Hunk {
    lines: Vec<String>,
}

fn parse_unified_patch(patch: &str) -> anyhow::Result<Vec<FilePatch>> {
    let mut file_patches = Vec::new();
    let mut current_old_path: Option<String> = None;
    let mut current_new_path: Option<String> = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_hunk: Option<Hunk> = None;

    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            continue;
        }
        if let Some(path) = line.strip_prefix("--- ") {
            current_old_path = Some(
                path.trim()
                    .strip_prefix("a/")
                    .unwrap_or(path.trim())
                    .to_string(),
            );
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            if let (Some(old_path), Some(new_path)) =
                (current_old_path.take(), current_new_path.take())
            {
                file_patches.push(FilePatch {
                    old_path,
                    new_path,
                    hunks,
                });
                hunks = Vec::new();
            }
            current_new_path = Some(
                path.trim()
                    .strip_prefix("b/")
                    .unwrap_or(path.trim())
                    .to_string(),
            );
            continue;
        }
        if line.starts_with("@@") {
            if current_new_path.is_none() {
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
    if let (Some(old_path), Some(new_path)) = (current_old_path.take(), current_new_path.take()) {
        file_patches.push(FilePatch {
            old_path,
            new_path,
            hunks,
        });
    }
    Ok(file_patches)
}

fn render_hunks_as_new_file(hunks: &[Hunk]) -> anyhow::Result<String> {
    let mut lines = Vec::new();
    for hunk in hunks {
        for line in &hunk.lines {
            if let Some(rest) = line.strip_prefix('+') {
                lines.push(rest.to_string());
            } else if line.starts_with(' ') || line.starts_with('-') {
                continue;
            } else {
                anyhow::bail!("unsupported hunk line for new file: {line}");
            }
        }
    }
    Ok(lines.join("\n"))
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
