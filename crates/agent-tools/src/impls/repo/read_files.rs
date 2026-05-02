use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde::Deserialize;
use serde_json::json;

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
