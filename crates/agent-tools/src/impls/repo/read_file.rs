use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_workspace_path};
use agent_core::ToolSpec;
use agent_core::ToolExecutionContext;
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
    max_chars: Option<usize>,
}

pub(crate) struct ReadFileLocalTool {
    pub(crate) max_read_chars: usize,
}

#[async_trait]
impl LocalTool for ReadFileLocalTool {
    fn spec(&self) -> ToolSpec {
        ReadFileTool::descriptor(self.max_read_chars).spec
    }
    async fn invoke(&self, arguments: Value, ctx: &ToolExecutionContext) -> Result<ToolInvocationOutput> {
        let args: ReadFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_workspace_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let text = fs::read_to_string(&path).await?;
        let max_chars = args.max_chars.unwrap_or(self.max_read_chars).max(128);
        let content = if text.chars().count() > max_chars {
            format!("{}\n\n[truncated]", text.chars().take(max_chars).collect::<String>())
        } else { text };
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
