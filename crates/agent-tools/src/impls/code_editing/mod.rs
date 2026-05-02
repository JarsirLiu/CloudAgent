use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde_json::json;

pub struct ApplyPatchTool;
pub struct WriteFileToolV2;

impl ApplyPatchTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::CodeEditing,
            ToolRisk::Medium,
            vec!["edit", "general"],
            ToolSpec {
                name: "apply_patch".to_string(),
                description: "Apply a focused patch to existing files. Prefer this over whole-file rewrites for code changes.".to_string(),
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

impl WriteFileToolV2 {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::CodeEditing,
            ToolRisk::High,
            vec!["edit", "fs", "general"],
            ToolSpec {
                name: "write_file".to_string(),
                description: "Create or replace a file when patch-based editing is not appropriate.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" },
                        "overwrite": { "type": "boolean" }
                    },
                    "required": ["path", "content"]
                }),
                mutating: true,
                requires_approval: true,
                item_kind: agent_protocol::TurnItemKind::FileChange,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: Some("Writing files can modify workspace contents.".to_string()),
            },
        )
    }
}
