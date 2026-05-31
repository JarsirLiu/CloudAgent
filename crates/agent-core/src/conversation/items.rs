use crate::tool::{CommandExecutionStatus, StructuredToolResult, WriteFileStatus};
use crate::turn::TurnState;
use serde::{Deserialize, Serialize};

use super::InputItem;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptItem {
    SystemMessage {
        id: String,
        text: String,
    },
    UserMessage {
        id: String,
        content: Vec<InputItem>,
    },
    AgentMessage {
        id: String,
        text: String,
    },
    CommandExecution {
        id: String,
        tool_name: String,
        command: String,
        current_directory: String,
        status: CommandExecutionStatus,
        exit_code: Option<i32>,
        output: Option<String>,
        duration_ms: Option<u64>,
        summary: String,
    },
    FileChange {
        id: String,
        tool_name: String,
        path: String,
        status: WriteFileStatus,
        files_changed: usize,
        summary: String,
    },
    ToolResult {
        id: String,
        tool_name: String,
        content: String,
        summary: String,
        structured: Option<StructuredToolResult>,
    },
    Reasoning {
        id: String,
        title: String,
        text: String,
    },
}

impl TranscriptItem {
    pub fn id(&self) -> &str {
        match self {
            Self::SystemMessage { id, .. }
            | Self::UserMessage { id, .. }
            | Self::AgentMessage { id, .. }
            | Self::CommandExecution { id, .. }
            | Self::FileChange { id, .. }
            | Self::ToolResult { id, .. }
            | Self::Reasoning { id, .. } => id,
        }
    }

    pub fn user_message(id: impl Into<String>, content: Vec<InputItem>) -> Self {
        Self::UserMessage {
            id: id.into(),
            content,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationTurn {
    pub id: String,
    pub state: TurnState,
    pub items: Vec<TranscriptItem>,
    pub rollout_start_index: usize,
    pub rollout_end_index: usize,
}
