use crate::impls::repo::text_read::{TextReadOptions, read_text_snippet};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolExecutionContext;
use agent_core::ToolSpec;
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::task::JoinSet;

pub struct ReadFilesTool;

#[derive(Debug, Clone, Deserialize)]
pub struct ReadFilesArgs {
    pub paths: Vec<String>,
    #[serde(default)]
    pub max_lines_per_file: Option<usize>,
}

impl ReadFilesTool {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "edit", "general"],
            ToolSpec {
                name: "read_files".to_string(),
                description: format!(
                    "Batch-read multiple candidate files in one round to reduce model roundtrips. Maximum characters per file are constrained by the workspace read limit of {max_read_chars}."
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        },
                        "max_lines_per_file": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["paths"]
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

pub(crate) struct ReadFilesLocalTool {
    pub(crate) max_read_chars: usize,
}

#[async_trait]
impl LocalTool for ReadFilesLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFilesTool::descriptor(self.max_read_chars).spec
    }
    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ReadFilesArgs = serde_json::from_value(arguments)?;
        if args.paths.is_empty() {
            bail!("`paths` must not be empty");
        }
        let options = TextReadOptions::for_batch_file(self.max_read_chars, args.max_lines_per_file);
        let workspace_root = ctx
            .workspace_root
            .canonicalize()
            .unwrap_or_else(|_| ctx.workspace_root.clone());
        let mut jobs = JoinSet::new();
        for requested_path in args.paths {
            let resolved =
                resolve_workspace_path(&ctx.workspace_root, Some(requested_path.as_str()))?;
            let workspace_root = workspace_root.clone();
            let display_path = resolved
                .strip_prefix(&workspace_root)
                .unwrap_or(&resolved)
                .to_string_lossy()
                .replace('\\', "/");
            let options = options.clone();
            jobs.spawn(async move {
                let read_result = read_text_snippet(&resolved, &options).await?;
                let content = match read_result {
                    Ok(text) => text.rendered,
                    Err(failure) => failure.render(),
                };
                Ok::<(String, String), anyhow::Error>((display_path, content))
            });
        }
        let mut blocks = Vec::new();
        while let Some(joined) = jobs.join_next().await {
            let (display_path, content) = joined??;
            blocks.push((display_path, content));
        }
        blocks.sort_by(|left, right| left.0.cmp(&right.0));
        let rendered = blocks
            .iter()
            .map(|(path, content)| format!("== {path} ==\n{content}"))
            .collect::<Vec<_>>();
        Ok(ToolInvocationOutput {
            content: rendered.join("\n\n"),
            structured: Some(agent_protocol::StructuredToolResult::ReadFiles {
                file_count: blocks.len(),
            }),
        })
    }
}
