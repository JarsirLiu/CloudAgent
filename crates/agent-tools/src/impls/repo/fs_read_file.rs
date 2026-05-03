use crate::impls::repo::text_read::{TextReadOptions, read_text_snippet};
use crate::registry::shared::{LocalTool, ToolInvocationOutput, resolve_read_path};
use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolExecutionContext;
use agent_core::ToolSpec;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

pub struct FsReadFileTool;

impl FsReadFileTool {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "edit", "verify", "general"],
            ToolSpec {
                name: "fs_read_file".to_string(),
                description: format!(
                    "Read a known file through the shared file access path. Use this after you already know the path. Do not use directory traversal to inspect file contents. Maximum characters per request: {max_read_chars}."
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
struct FsReadFileArgs {
    path: String,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    max_lines: Option<usize>,
}

pub(crate) struct FsReadFileLocalTool {
    pub(crate) max_read_chars: usize,
}

#[async_trait]
impl LocalTool for FsReadFileLocalTool {
    fn spec(&self) -> ToolSpec {
        FsReadFileTool::descriptor(self.max_read_chars).spec
    }
    async fn invoke(
        &self,
        arguments: Value,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: FsReadFileArgs = serde_json::from_value(arguments)?;
        let path = resolve_read_path(&ctx.workspace_root, Some(args.path.as_str()))?;
        let read_result = read_text_snippet(
            &path,
            &TextReadOptions::for_single_file(self.max_read_chars, args.start_line, args.max_lines),
        )
        .await?;
        let (content, truncated, char_count) = match read_result {
            Ok(text) => (text.rendered, text.truncated, text.source_char_count),
            Err(failure) => (failure.render(), false, 0),
        };
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
