use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde_json::json;

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
