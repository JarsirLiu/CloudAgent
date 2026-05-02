use crate::spec::{ToolCategory, ToolDescriptor, ToolRisk};
use agent_core::ToolSpec;
use serde_json::json;

pub struct ShellCommandToolV2;

impl ShellCommandToolV2 {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new(
            ToolCategory::CommandExecution,
            ToolRisk::High,
            vec!["explore", "edit", "verify", "general"],
            ToolSpec {
                name: "shell_command".to_string(),
                description: "Run a local shell command for build, test, git, or high-density workspace inspection.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "workdir": { "type": "string" },
                        "timeout_ms": { "type": "integer", "minimum": 1000 }
                    },
                    "required": ["command"]
                }),
                mutating: true,
                requires_approval: true,
                item_kind: agent_protocol::TurnItemKind::CommandExecution,
                delta_kind: agent_protocol::TurnItemDeltaKind::CommandExecutionOutput,
                approval_reason: Some("Shell commands can inspect or modify the workspace.".to_string()),
            },
        )
    }
}
