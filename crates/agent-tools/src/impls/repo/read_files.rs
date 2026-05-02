use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::ToolSpec;
use agent_core::ToolExecutionContext;
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::fs;

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
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadFilesArgs = serde_json::from_value(arguments)?;
        if args.paths.is_empty() { bail!("`paths` must not be empty"); }
        let max_lines = args.max_lines_per_file.unwrap_or(300).clamp(1, 2_000);
        let mut blocks = Vec::new();
        for path in args.paths {
            let resolved = resolve_workspace_path(&ctx.workspace_root, Some(path.as_str()))?;
            let text = fs::read_to_string(&resolved).await?;
            let mut lines = Vec::new();
            for (idx, line) in text.lines().enumerate() {
                if idx >= max_lines { lines.push("[truncated]".to_string()); break; }
                lines.push(line.to_string());
            }
            let rel = resolved.strip_prefix(&ctx.workspace_root).unwrap_or(&resolved).to_string_lossy().replace('\\', "/");
            blocks.push(format!("== {} ==\n{}", rel, lines.join("\n")));
        }
        Ok(ToolInvocationOutput {
            content: blocks.join("\n\n"),
            structured: Some(agent_protocol::StructuredToolResult::ReadFiles {
                file_count: blocks.len(),
            }),
        })
    }
}
