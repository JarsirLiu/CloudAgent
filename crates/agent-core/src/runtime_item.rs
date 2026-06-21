use crate::conversation::TranscriptItem;
use crate::tool::{StructuredToolResult, ToolIdentity};
use crate::turn::TurnItemKind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeItemStatus {
    InProgress,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeItemMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_read: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_written: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_count: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeItemProgress {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeItem {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub kind: TurnItemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub status: RuntimeItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_identity: Option<ToolIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<StructuredToolResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<RuntimeItemProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<RuntimeItemMetrics>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeItemSnapshot {
    pub item: RuntimeItem,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text_buffer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning_buffer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_output_buffer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub patch_buffer: String,
}

impl RuntimeItem {
    pub fn started(
        id: impl Into<String>,
        call_id: Option<String>,
        kind: TurnItemKind,
        title: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            call_id,
            kind,
            title,
            status: RuntimeItemStatus::InProgress,
            summary: None,
            tool_identity: None,
            structured: None,
            progress: None,
            metrics: None,
        }
    }

    pub fn completed(transcript_item: &TranscriptItem, call_id: Option<String>) -> Self {
        match transcript_item {
            TranscriptItem::SystemMessage { id, text } => Self {
                id: id.clone(),
                call_id,
                kind: TurnItemKind::SystemNote,
                title: None,
                status: RuntimeItemStatus::Completed,
                summary: Some(text.clone()),
                tool_identity: None,
                structured: None,
                progress: None,
                metrics: None,
            },
            TranscriptItem::UserMessage { id, .. } => Self {
                id: id.clone(),
                call_id,
                kind: TurnItemKind::UserMessage,
                title: None,
                status: RuntimeItemStatus::Completed,
                summary: None,
                tool_identity: None,
                structured: None,
                progress: None,
                metrics: None,
            },
            TranscriptItem::AgentMessage { id, text } => Self {
                id: id.clone(),
                call_id,
                kind: TurnItemKind::AssistantMessage,
                title: Some("assistant_message".to_string()),
                status: RuntimeItemStatus::Completed,
                summary: Some(text.clone()),
                tool_identity: None,
                structured: None,
                progress: None,
                metrics: None,
            },
            TranscriptItem::CommandExecution {
                id,
                command,
                summary,
                duration_ms,
                ..
            } => Self {
                id: id.clone(),
                call_id,
                kind: TurnItemKind::CommandExecution,
                title: Some(command.clone()),
                status: RuntimeItemStatus::Completed,
                summary: Some(summary.clone()),
                tool_identity: None,
                structured: None,
                progress: None,
                metrics: Some(RuntimeItemMetrics {
                    input_tokens: None,
                    output_tokens: None,
                    total_tokens: None,
                    elapsed_ms: *duration_ms,
                    bytes_read: None,
                    bytes_written: None,
                    file_count: None,
                    source_count: None,
                    result_count: None,
                }),
            },
            TranscriptItem::FileChange {
                id,
                path,
                files_changed,
                summary,
                ..
            } => Self {
                id: id.clone(),
                call_id,
                kind: TurnItemKind::FileChange,
                title: Some(path.clone()),
                status: RuntimeItemStatus::Completed,
                summary: Some(summary.clone()),
                tool_identity: None,
                structured: None,
                progress: None,
                metrics: Some(RuntimeItemMetrics {
                    input_tokens: None,
                    output_tokens: None,
                    total_tokens: None,
                    elapsed_ms: None,
                    bytes_read: None,
                    bytes_written: None,
                    file_count: Some(*files_changed),
                    source_count: None,
                    result_count: None,
                }),
            },
            TranscriptItem::ToolResult {
                id,
                tool_name,
                summary,
                structured,
                ..
            } => Self {
                id: id.clone(),
                call_id,
                kind: TurnItemKind::ToolResult,
                title: Some(tool_name.clone()),
                status: RuntimeItemStatus::Completed,
                summary: Some(summary.clone()),
                tool_identity: None,
                structured: structured.clone(),
                progress: None,
                metrics: RuntimeItemMetrics::from_structured_result(structured.as_ref()),
            },
            TranscriptItem::Reasoning { id, title, text } => Self {
                id: id.clone(),
                call_id,
                kind: TurnItemKind::Reasoning,
                title: Some(title.clone()),
                status: RuntimeItemStatus::Completed,
                summary: Some(text.clone()),
                tool_identity: None,
                structured: None,
                progress: None,
                metrics: None,
            },
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        let summary = summary.into();
        self.summary = (!summary.trim().is_empty()).then_some(summary);
        self
    }

    pub fn with_tool_identity(mut self, identity: ToolIdentity) -> Self {
        self.tool_identity = Some(identity);
        self
    }

    pub fn with_structured(mut self, structured: StructuredToolResult) -> Self {
        self.structured = Some(structured);
        self
    }

    pub fn with_progress(mut self, progress: RuntimeItemProgress) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn with_metrics(mut self, metrics: RuntimeItemMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }
}

impl RuntimeItemProgress {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            completed: None,
            total: None,
            unit: None,
        }
    }
}

impl RuntimeItemMetrics {
    pub fn from_transcript_item(item: &TranscriptItem) -> Option<Self> {
        match item {
            TranscriptItem::CommandExecution { duration_ms, .. } => Some(Self {
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                elapsed_ms: *duration_ms,
                bytes_read: None,
                bytes_written: None,
                file_count: None,
                source_count: None,
                result_count: None,
            }),
            TranscriptItem::FileChange { files_changed, .. } => Some(Self {
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                elapsed_ms: None,
                bytes_read: None,
                bytes_written: None,
                file_count: Some(*files_changed),
                source_count: None,
                result_count: None,
            }),
            TranscriptItem::ToolResult { structured, .. } => {
                Self::from_structured_result(structured.as_ref())
            }
            _ => None,
        }
    }

    pub fn from_structured_result(structured: Option<&StructuredToolResult>) -> Option<Self> {
        match structured {
            Some(StructuredToolResult::WebSearch {
                result_count,
                source_count,
                ..
            }) => Some(Self {
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                elapsed_ms: None,
                bytes_read: None,
                bytes_written: None,
                file_count: None,
                source_count: *source_count,
                result_count: *result_count,
            }),
            Some(StructuredToolResult::CommandExecution { duration_ms, .. }) => Some(Self {
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                elapsed_ms: *duration_ms,
                bytes_read: None,
                bytes_written: None,
                file_count: None,
                source_count: None,
                result_count: None,
            }),
            Some(StructuredToolResult::EditFile { files_changed, .. }) => Some(Self {
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                elapsed_ms: None,
                bytes_read: None,
                bytes_written: None,
                file_count: Some(*files_changed),
                source_count: None,
                result_count: None,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "runtime_item_tests.rs"]
mod tests;
