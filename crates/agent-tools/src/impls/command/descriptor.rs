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
                    "repository search with `rg` or `rg --files`",
                    "targeted file inspection with platform shell commands",
                    "build, test, git, and runtime verification",
                    "long-running noninteractive commands that can be polled with write_stdin",
                ],
                avoid_for: vec![
                    "workspace file edits",
                    "full-screen interactive programs such as editors, pagers, shells, REPLs, SSH, or TUI apps",
                ],
                follow_up_hint: Some("prefer `workdir` over inline `cd`; use noninteractive flags; on Windows use PowerShell syntax"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "exec_command".to_string(),
                identity: ToolIdentity::built_in("exec_command"),
                description: "Run a local noninteractive shell command for repository search, file inspection, build, test, git, and runtime verification. Use apply_patch for workspace file edits. Avoid full-screen interactive programs such as editors, pagers, shells, REPLs, SSH, or TUI apps. If the command is still running after yield_time_ms, the runtime returns a session_id for polling or simple stdin responses with write_stdin.".to_string(),
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
                        "yield_time_ms": {
                            "type": "integer",
                            "minimum": 1000,
                            "description": "Maximum time to wait for this invocation."
                        },
                        "max_output_tokens": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of tokens to return. Excess output is truncated."
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
                    "sending a simple response to an existing command session",
                    "polling output from a still-running command session",
                ],
                avoid_for: vec![
                    "starting new commands",
                    "driving full-screen interactive programs, shells, editors, pagers, REPLs, SSH, or TUI apps",
                ],
                follow_up_hint: Some("use only with a session_id returned by exec_command; prefer empty chars for polling"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "write_stdin".to_string(),
                identity: ToolIdentity::built_in("write_stdin"),
                description: "Write a simple stdin response to an existing command session, or pass an empty string to poll for new output. Do not use this to drive full-screen interactive programs, shells, editors, pagers, REPLs, SSH, or TUI apps.".to_string(),
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
                        "yield_time_ms": {
                            "type": "integer",
                            "minimum": 1000,
                            "description": "Maximum time to wait for this invocation."
                        },
                        "max_output_tokens": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of tokens to return. Excess output is truncated."
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
