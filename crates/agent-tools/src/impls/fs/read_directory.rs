use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_workspace_path,
};
use crate::spec::{
    ToolCategory, ToolDefaultVisibility, ToolDescriptor, ToolPermissionTier, ToolRisk,
    ToolUsageGuidance,
};
use agent_core::{ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tokio::fs;

pub struct ReadDirectoryTool;

impl ReadDirectoryTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["explore", "verify", "fs"],
            ToolUsageGuidance {
                selection_priority: 6,
                preferred_for: vec![
                    "checking the contents of one known directory",
                    "verifying generated files after a write or build step",
                ],
                avoid_for: vec![
                    "broad repository discovery",
                    "finding source files by fuzzy name or symbol",
                ],
                follow_up_hint: Some(
                    "use `search_workspace` for source discovery and `read_file` for file contents",
                ),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "read_directory".to_string(),
                identity: ToolIdentity::built_in("read_directory"),
                description:
                    "Read one known directory and return its immediate entries in a structured result."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "max_entries": { "type": "integer", "minimum": 1, "maximum": 500 }
                    },
                    "required": ["path"]
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::ParallelSafe,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
        .with_default_visibility(ToolDefaultVisibility::Deferred)
    }
}

#[derive(Debug, Deserialize)]
struct ReadDirectoryArgs {
    path: String,
    #[serde(default)]
    max_entries: Option<usize>,
}

pub(crate) struct ReadDirectoryLocalTool;

#[async_trait]
impl LocalTool for ReadDirectoryLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadDirectoryTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ReadDirectoryArgs = invocation.payload.parse_arguments()?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let max_entries = args.max_entries.unwrap_or(200).clamp(1, 500);

        let mut entries = Vec::new();
        let mut dir = fs::read_dir(&path).await?;
        while let Some(entry) = dir.next_entry().await? {
            let entry_path = entry.path();
            let file_type = entry.file_type().await?;
            entries.push(agent_protocol::DirectoryEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry_path.display().to_string(),
                is_file: file_type.is_file(),
                is_dir: file_type.is_dir(),
                is_symlink: file_type.is_symlink(),
            });
        }
        entries.sort_by(|left, right| {
            left.is_file
                .cmp(&right.is_file)
                .then_with(|| left.name.cmp(&right.name))
        });

        let truncated = entries.len() > max_entries;
        let shown_entries = entries.into_iter().take(max_entries).collect::<Vec<_>>();
        let content = if shown_entries.is_empty() {
            format!("Directory `{}` is empty.", path.display())
        } else {
            let listing = shown_entries
                .iter()
                .map(|entry| {
                    let kind = if entry.is_dir { "dir" } else { "file" };
                    format!("{kind}\t{}", entry.name)
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{} entries under `{}`:\n{listing}",
                shown_entries.len(),
                path.display()
            )
        };

        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::ReadDirectory {
                path: path.display().to_string(),
                entry_count: shown_entries.len(),
                truncated,
                entries: shown_entries,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::shared::{LocalToolPayload, LocalToolSource};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn read_directory_returns_immediate_entries() {
        let base = test_workspace("read_directory_returns_immediate_entries");
        fs::create_dir_all(base.join("src/nested"))
            .await
            .expect("create nested");
        fs::write(base.join("src/lib.rs"), "pub fn demo() {}\n")
            .await
            .expect("write file");

        let tool = ReadDirectoryLocalTool;
        let output = tool
            .invoke(
                LocalToolInvocation {
                    identity: ToolIdentity::built_in("read_directory"),
                    source: LocalToolSource::BuiltIn,
                    payload: LocalToolPayload::Function {
                        arguments: json!({
                            "path": "src"
                        }),
                    },
                },
                &tool_context(&base),
            )
            .await
            .expect("read_directory works");

        assert!(output.content.contains("entries under"));
        assert!(matches!(
            output.structured.as_ref(),
            Some(agent_protocol::StructuredToolResult::ReadDirectory {
                entry_count,
                entries,
                ..
            }) if *entry_count == 2
                && entries.iter().any(|entry| entry.name == "lib.rs" && entry.is_file)
                && entries.iter().any(|entry| entry.name == "nested" && entry.is_dir)
        ));
    }

    fn tool_context(workspace_root: &std::path::Path) -> agent_core::ToolExecutionContext {
        agent_core::ToolExecutionContext {
            conversation_id: "test".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            conversation_store_dir: workspace_root.to_path_buf(),
            permission_profile: agent_core::PermissionProfile::ReadOnly,
            default_shell_timeout_ms: 5_000,
            cancellation_token: CancellationToken::new(),
            discoverable_tools: Vec::new(),
            output_tx: None,
        }
    }

    fn test_workspace(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_millis();
        path.push(format!("cloudagent_{name}_{stamp}"));
        std::fs::create_dir_all(&path).expect("create temp workspace");
        path
    }
}

