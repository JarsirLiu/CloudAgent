use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde_json::json;

pub struct GetMetadataTool;

impl GetMetadataTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::WorkspaceFileOps,
            ToolRisk::Low,
            vec!["explore", "verify", "fs", "general"],
            ToolSpec {
                name: "get_metadata".to_string(),
                description: "Read path metadata such as existence, type, size, and modification time."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
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
