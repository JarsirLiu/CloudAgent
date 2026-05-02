use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde_json::json;

pub struct ApplyPatchTool;

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
