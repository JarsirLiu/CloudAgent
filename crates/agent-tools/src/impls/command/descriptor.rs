use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{ToolExecutionPolicy, ToolIdentity, ToolSpec, TurnItemDeltaKind, TurnItemKind};
use serde_json::json;

pub struct ExecCommandTool;
pub struct WriteStdinTool;

impl ExecCommandTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::CommandExecution,
            ToolRisk::High,
            ToolPermissionTier::ReadOnly,
            vec!["edit", "verify"],
            ToolUsageGuidance {
                selection_priority: 15,
                preferred_for: vec![
                    "build, test, git, and runtime verification",
                    "interactive command sessions",
                ],
                avoid_for: vec![
                    "workspace file edits",
                    "repository search when structured tools are available",
                ],
                follow_up_hint: Some("prefer `workdir` over inline `cd`; on Windows use PowerShell syntax"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "exec_command".to_string(),
                identity: ToolIdentity::built_in("exec_command"),
                description: "Run a local command for build, test, git, and runtime verification. If the command is still running after timeout_ms, the runtime returns a session_id for follow-up write_stdin calls.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Shell command to execute."
                        },
                        "workdir": {
                            "type": "string",
                            "description": "Working directory for the command. Prefer this over inline cd."
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "minimum": 1000,
                            "description": "Maximum time to wait for this invocation."
                        }
                    },
                    "required": ["command"],
                    "additionalProperties": false
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: true,
                item_kind: TurnItemKind::CommandExecution,
                delta_kind: TurnItemDeltaKind::CommandExecutionOutput,
                approval_reason: Some(
                    "Local command execution can inspect or modify the workspace.".to_string(),
                ),
            },
        )
    }
}

impl WriteStdinTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::CommandExecution,
            ToolRisk::High,
            ToolPermissionTier::ReadOnly,
            vec!["interact", "poll"],
            ToolUsageGuidance {
                selection_priority: 9,
                preferred_for: vec![
                    "sending input to an existing command session",
                    "polling output from a still-running command session",
                ],
                avoid_for: vec!["starting new commands"],
                follow_up_hint: Some("use only with a session_id returned by exec_command"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "write_stdin".to_string(),
                identity: ToolIdentity::built_in("write_stdin"),
                description: "Write characters to an existing command session, or pass an empty string to poll for new output.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "Session id returned by an earlier exec_command result."
                        },
                        "chars": {
                            "type": "string",
                            "description": "Characters to write to stdin. Use an empty string to poll for new output without writing."
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "minimum": 1000,
                            "description": "Maximum time to wait for this invocation."
                        }
                    },
                    "required": ["session_id", "chars"],
                    "additionalProperties": false
                }),
                mutating: true,
                execution_policy: ToolExecutionPolicy::Sequential,
                requires_approval: true,
                item_kind: TurnItemKind::CommandExecution,
                delta_kind: TurnItemDeltaKind::CommandExecutionOutput,
                approval_reason: Some(
                    "Writing to an interactive command session can affect the workspace."
                        .to_string(),
                ),
            },
        )
    }
}
