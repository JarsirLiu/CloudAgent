use crate::v2::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde_json::json;

pub struct SearchTextTool;
pub struct FindFilesTool;
pub struct ReadFileToolV2;
pub struct ReadFilesTool;

impl SearchTextTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "general"],
            ToolSpec {
                name: "search_text".to_string(),
                description: "Search workspace text by keyword or regex. Prefer this over directory-by-directory traversal when locating implementations.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "path_scope": { "type": "string" },
                        "file_glob": { "type": "string" },
                        "regex": { "type": "boolean" },
                        "case_sensitive": { "type": "boolean" },
                        "max_results": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["query"]
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

impl FindFilesTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "general"],
            ToolSpec {
                name: "find_files".to_string(),
                description: "Find candidate files by name, extension, or glob pattern. Use this before broad directory walking.".to_string(),
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

impl ReadFileToolV2 {
    pub fn descriptor(max_read_chars: usize) -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            vec!["explore", "repo", "edit", "verify", "general"],
            ToolSpec {
                name: "read_file_v2".to_string(),
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
