use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde::Deserialize;
use serde_json::json;

pub struct FindFilesTool;

#[derive(Debug, Clone, Deserialize)]
pub struct FindFilesArgs {
    pub pattern: String,
    #[serde(default)]
    pub path_scope: Option<String>,
    #[serde(default)]
    pub max_results: Option<usize>,
}

impl FindFilesTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "general"],
            ToolSpec {
                name: "find_files".to_string(),
                description: "Find candidate files by name, extension, or glob pattern. Use this before broad directory walking. By default, file discovery should respect ignore rules and skip common dependency and build output directories such as .git, node_modules, dist, and target.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string" },
                        "path_scope": { "type": "string" },
                        "max_results": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["pattern"]
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
