use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolExecutionContext;
use agent_core::ToolSpec;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::fs;

pub struct ReadFileTool;

impl ReadFileTool {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "edit", "verify", "general"],
            ToolSpec {
                name: "read_file".to_string(),
                description: format!(
                    "Read a known file with optional line offsets. Use this for focused inspection after locating candidate files. Maximum characters per request: {max_read_chars}."
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "start_line": { "type": "integer", "minimum": 1 },
                        "max_lines": { "type": "integer", "minimum": 1 }
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
struct ReadFileArgs {
    path: String,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    max_lines: Option<usize>,
}

pub(crate) struct ReadFileLocalTool {
    pub(crate) max_read_chars: usize,
}

#[async_trait]
impl LocalTool for ReadFileLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFileTool::descriptor(self.max_read_chars).spec
    }
    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: ReadFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let text = fs::read_to_string(&path).await?;
        let start_line = args.start_line.unwrap_or(1).max(1);
        let max_lines = args.max_lines.unwrap_or(500).clamp(1, 5000);
        let selected = text
            .lines()
            .skip(start_line.saturating_sub(1))
            .take(max_lines)
            .collect::<Vec<_>>()
            .join("\n");
        let max_chars = self.max_read_chars.max(128);
        let content = if selected.chars().count() > max_chars {
            format!(
                "{}\n\n[truncated]",
                selected.chars().take(max_chars).collect::<String>()
            )
        } else {
            selected
        };
        let char_count = content.chars().count();
        let truncated = content.ends_with("\n\n[truncated]");
        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::ReadFile {
                path: path.display().to_string(),
                truncated,
                char_count,
            }),
        })
    }
}
