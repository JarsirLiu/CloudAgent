use super::common::DEFAULT_IGNORED_DIRS;
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

pub struct ListDirectoryTool;

#[derive(Debug, Clone, Deserialize)]
struct ListDirectoryArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    recursive: Option<bool>,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

impl ListDirectoryTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            true,
            vec!["explore", "repo", "fs"],
            ToolUsageGuidance {
                selection_priority: 10,
                preferred_for: vec!["lightweight path discovery", "small tree inspection"],
                avoid_for: vec![
                    "root-cause analysis when target files are not yet known",
                    "repeated broad repo wandering",
                ],
                preferred_task_kinds: vec![
                    agent_core::TaskKind::RepositoryAnalysis,
                    agent_core::TaskKind::WorkspaceFileOperation,
                ],
                follow_up_hint: Some(
                    "switch to `search_workspace` for bug investigation or `read_files` once paths are known",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "list_directory".to_string(),
                identity: ToolIdentity::built_in("list_directory"),
                description: "List workspace directory entries without dropping to the shell."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "recursive": { "type": "boolean" },
                        "max_results": { "type": "integer", "minimum": 1 },
                        "offset": { "type": "integer", "minimum": 0 }
                    }
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::ParallelSafe,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
    }
}

pub(crate) struct ListDirectoryLocalTool;

#[async_trait]
impl LocalTool for ListDirectoryLocalTool {
    fn spec(&self) -> ToolSpec {
        ListDirectoryTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ListDirectoryArgs = invocation.payload.parse_arguments()?;
        let root = resolve_read_path(&ctx.workspace_root, args.path.as_deref())?;
        let recursive = args.recursive.unwrap_or(false);
        let max_results = args.max_results.unwrap_or(200).clamp(1, 2_000);
        let offset = args.offset.unwrap_or(0);
        let workspace_root = ctx.workspace_root.clone();
        let root_for_walk = root.clone();
        let display_path = root
            .strip_prefix(&workspace_root)
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| ".".to_string());

        let entries = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            if recursive {
                collect_recursive_entries(&workspace_root, &root_for_walk)
            } else {
                collect_shallow_entries(&workspace_root, &root_for_walk)
            }
        })
        .await??;

        let total_entries = entries.len();
        let displayed = entries
            .into_iter()
            .skip(offset)
            .take(max_results)
            .collect::<Vec<_>>();
        let shown_count = displayed.len();
        let truncated = total_entries > offset.saturating_add(shown_count);

        let content = if displayed.is_empty() {
            format!("No entries found under {display_path}.")
        } else {
            let mut content = format!(
                "Entries for {display_path} (showing {} of {total_entries}):\n{}",
                shown_count,
                displayed.join("\n")
            );
            if truncated {
                content.push_str(&format!(
                    "\n…and {} more",
                    total_entries.saturating_sub(offset.saturating_add(shown_count))
                ));
            }
            content
        };

        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::ListDirectory {
                path: root.display().to_string(),
                recursive,
                offset,
                shown_count,
                total_count: total_entries,
                truncated,
            }),
        })
    }
}

fn collect_shallow_entries(workspace_root: &Path, root: &Path) -> Result<Vec<String>> {
    let mut entries = std::fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if DEFAULT_IGNORED_DIRS.contains(&file_name.as_str()) {
                return None;
            }
            let relative = entry
                .path()
                .strip_prefix(workspace_root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let suffix = if entry.file_type().ok()?.is_dir() {
                "/"
            } else {
                ""
            };
            Some(format!("{relative}{suffix}"))
        })
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn collect_recursive_entries(workspace_root: &Path, root: &Path) -> Result<Vec<String>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .follow_links(false);
    builder.filter_entry(|entry| {
        let name = entry.file_name().to_string_lossy();
        !DEFAULT_IGNORED_DIRS.contains(&name.as_ref())
    });

    let mut entries = Vec::new();
    for result in builder.build() {
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path == root {
            continue;
        }
        let Ok(relative) = path.strip_prefix(workspace_root) else {
            continue;
        };
        let mut rendered = relative.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            rendered.push('/');
        }
        entries.push(rendered);
    }
    entries.sort();
    Ok(entries)
}
